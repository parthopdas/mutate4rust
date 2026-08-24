//! Command-line adapter for `mutate4rust`.
//!
//! This module is an **infrastructure/adapter** concern living at the outer edge
//! of the crate (per the T1 design review): it parses process arguments with
//! `clap` and dispatches into the engine. The pure core (mutation-site model,
//! operator mappings) must never depend on this module — the dependency arrow
//! points inward, from adapters toward the core.
//!
//! During the S1 bootstrap the flag surface below was **parse-only**: every option
//! mirrors mutate4go's CLI for 1:1 muscle-memory parity (feature file C10). Scan,
//! manifest, and the mutate loop are now wired; coverage gating and the remaining
//! execution flags land in later slices.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::Parser;

use crate::manifest::{self, CURRENT_SCHEMA_VERSION, INTERIM_HASHER_ID, Manifest};
use crate::pipeline;
use crate::report::MutationReport;
use crate::runner;
use crate::scanner;

/// Default mutation-count warning threshold (feature file `--scan`/T6 note).
const DEFAULT_MUTATION_WARNING: usize = 50;

/// Changed-site count reported by `--scan` until differential hashing lands.
///
/// The `--scan` task text pins this to `0`: per-function hash diffing isn't built
/// yet, so the mode reports a deterministic stub rather than a real changed count.
//
// TODO(S7/T14): compute real changed-site count via per-function hash diff.
const STUB_CHANGED_SITES: usize = 0;

/// Exit-code contract for `mutate4rust` — **strict mutate4go parity** (decision C11).
///
/// The codes are stable and documented so pipelines can branch on them:
///
/// - `0` — **success**: the run (or scan) completed. This **includes runs where
///   mutants survive** — survivors are reported, not signalled by the exit code
///   (mutate4rust does *not* fail CI on survivors).
/// - `1` — **error**: any failure — invalid command-line usage *and* operational
///   failures (file I/O, `syn` parse, coverage tooling, or the test runner) both
///   map here. Upstream `runner.StatusCode` returns 1 on any error and does not
///   distinguish usage from operational; clap's default usage exit (`2`) is
///   overridden to `1` (see [`Cli::main`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitStatus {
    /// The run completed — surviving mutants are reported, not signalled here.
    Success,
    /// Any failure — usage or operational.
    Error,
}

impl ExitStatus {
    /// The raw exit code defined by the contract above.
    fn raw_code(self) -> u8 {
        match self {
            ExitStatus::Success => 0,
            ExitStatus::Error => 1,
        }
    }

    /// Maps the outcome onto the documented process exit code.
    fn code(self) -> ExitCode {
        ExitCode::from(self.raw_code())
    }
}

/// The resolved run mode (design.md → `Mode` enum at the CLI boundary).
///
/// Precedence mirrors upstream: `--scan` wins, else `--update-manifest`, else a
/// normal mutation run.
//
// TODO(S7/T15): enforce mutual exclusivity via clap ArgGroup — for now precedence
// is resolved deterministically here rather than rejecting conflicting flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Report mutation-site counts without running tests (`--scan`).
    Scan,
    /// Refresh the sidecar manifest (`--update-manifest`).
    UpdateManifest,
    /// A normal mutation run.
    Mutate,
}

impl Mode {
    /// Resolves the mode from parsed flags with deterministic precedence.
    fn resolve(cli: &Cli) -> Mode {
        if cli.scan {
            Mode::Scan
        } else if cli.update_manifest {
            Mode::UpdateManifest
        } else {
            Mode::Mutate
        }
    }
}

/// A deterministic, greppable `--scan` report, computed purely from the scan
/// counts and threshold — deliberately kept free of I/O so its output shape and
/// warning logic can be unit-tested directly, while [`Cli::run_scan`] does the
/// file/sidecar reads and routes the rendered text to the right stream.
struct ScanReport {
    /// The scanned file, as displayed to the user (`self.file.display()`).
    file: String,
    /// Total in-scope mutation sites discovered by [`scanner::scan_source`].
    total_sites: usize,
    /// Sites changed since the manifest — the [`STUB_CHANGED_SITES`] stub for now.
    changed_sites: usize,
    /// `--mutation-warning` threshold; a strictly greater total is advisory-warned.
    warn_threshold: usize,
}

impl ScanReport {
    /// Whether the site total strictly exceeds the warning threshold, in which
    /// case an advisory warning fires. Equal-to-threshold does **not** warn.
    fn exceeds_threshold(&self) -> bool {
        self.total_sites > self.warn_threshold
    }

    /// The primary report body for stdout: a stable, greppable multi-line summary
    /// (trailing newline included) suitable for snapshot testing.
    fn summary(&self) -> String {
        format!(
            "Scanning {}...\n\
             Mutation sites: {}\n\
             Changed sites: {} (differential not yet active; reported as 0 until S7)\n",
            self.file, self.total_sites, self.changed_sites,
        )
    }

    /// The advisory over-threshold warning line, or `None` when at/below threshold.
    ///
    /// Advisory only — `--scan` is read-only, so a large count is not an error
    /// (C11); this just helps users avoid accidentally launching a huge run.
    fn warning(&self) -> Option<String> {
        self.exceeds_threshold().then(|| {
            format!(
                "warning: {} mutation sites exceed the --mutation-warning threshold ({}); \
                 this may be a large mutation run",
                self.total_sites, self.warn_threshold,
            )
        })
    }
}

/// `mutate4rust` — a single-file Rust mutation tester with mutate4go CLI parity.
///
/// Discovers mutation sites in one `.rs` file, applies each mutation, runs the
/// crate's tests, and reports killed / survived / uncovered mutants.
#[derive(Debug, Parser)]
#[command(name = "mutate4rust", version, about, long_about = None)]
pub struct Cli {
    /// Target Rust source file to mutate (one file at a time).
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Report mutation-site counts (total + changed) without running any tests.
    #[arg(long)]
    pub scan: bool,

    /// Refresh the sidecar manifest (`<file>.rs.m4r.toml`) after the run.
    #[arg(long)]
    pub update_manifest: bool,

    /// Restrict mutation to specific 1-based line numbers (comma-separated, e.g. `45,67`).
    #[arg(long, value_name = "LINES")]
    pub lines: Option<String>,

    /// Only mutate functions changed since the last recorded run (differential).
    #[arg(long)]
    pub since_last_run: bool,

    /// Mutate every site, overriding differential selection.
    #[arg(long)]
    pub mutate_all: bool,

    /// Reuse an existing coverage profile instead of regenerating one.
    #[arg(long)]
    pub reuse_coverage: bool,

    /// Warn when the number of mutation sites exceeds this threshold.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_MUTATION_WARNING)]
    pub mutation_warning: usize,

    /// Per-mutant timeout as a multiple of the measured baseline test phase.
    #[arg(long, value_name = "N")]
    pub timeout_factor: Option<f64>,

    /// Override the test command used to classify mutants (default: `cargo test`).
    #[arg(long, value_name = "CMD")]
    pub test_command: Option<String>,

    /// Number of parallel mutation workers (default: single worker).
    #[arg(long, value_name = "N")]
    pub max_workers: Option<usize>,

    /// Emit verbose progress output.
    #[arg(long)]
    pub verbose: bool,
}

impl Cli {
    /// Parses process arguments and runs the CLI, returning a process exit code.
    ///
    /// `clap` serves `--help`/`--version` itself. On a **usage error** clap would
    /// normally exit with code `2`; for strict mutate4go parity (decision C11) we
    /// intercept parsing and map any usage error onto exit `1`, while help/version
    /// still exit `0`. Successful parses flow through [`Self::run`].
    #[must_use]
    pub fn main() -> ExitCode {
        match Self::try_parse() {
            Ok(cli) => cli.run().code(),
            Err(err) => {
                // Renders the message (stderr for errors, stdout for help/version).
                let _ = err.print();
                Self::exit_status_for_parse_error(&err).code()
            }
        }
    }

    /// Maps a clap parse outcome onto the parity exit-code contract: help/version
    /// requests are a clean `Success` (0); every genuine usage error is `Error`
    /// (1), overriding clap's default usage code of `2`.
    fn exit_status_for_parse_error(err: &clap::Error) -> ExitStatus {
        if err.use_stderr() {
            ExitStatus::Error
        } else {
            ExitStatus::Success
        }
    }

    /// Dispatches the parsed options to the appropriate handler by resolved [`Mode`].
    ///
    /// Returns the [`ExitStatus`] outcome; [`Self::main`] applies the thin
    /// [`ExitStatus::code`] wrapper to turn it into a process [`ExitCode`].
    /// Returning the richer status (rather than an already-collapsed `ExitCode`)
    /// keeps dispatch routing directly assertable in tests.
    #[must_use]
    fn run(self) -> ExitStatus {
        match Mode::resolve(&self) {
            Mode::Scan => self.run_scan(),
            Mode::UpdateManifest => self.run_update_manifest(),
            Mode::Mutate => self.run_mutation(),
        }
    }

    /// Reports mutation-site counts for `--scan` without running tests or applying
    /// mutations — a **read-only** mode (mutate4go parity, feature file C11).
    ///
    /// Reads the target source from disk (infrastructure) and the sidecar manifest
    /// (tolerating a first-run absence, [`manifest::read`] → `Ok(None)`), then
    /// derives a pure [`ScanReport`]. The primary summary goes to stdout; the
    /// over-threshold advisory (if any) goes to stderr. Any I/O or parse failure
    /// maps onto [`ExitStatus::Error`] (C11: exit `1`); a successful scan — even a
    /// large one that triggers the warning — is [`ExitStatus::Success`] (exit `0`).
    fn run_scan(&self) -> ExitStatus {
        match self.scan_report() {
            Ok(report) => {
                print!("{}", report.summary());
                if let Some(warning) = report.warning() {
                    eprintln!("{warning}");
                }
                ExitStatus::Success
            }
            Err(err) => {
                eprintln!("mutate4rust: {err:#}");
                ExitStatus::Error
            }
        }
    }

    /// Reads the source and sidecar and assembles the pure [`ScanReport`].
    ///
    /// Reading the source from disk and the sidecar are infrastructure work done
    /// here; site counting is delegated to the pure [`scanner::scan_source`] seam.
    /// A missing sidecar is the normal first-run case ([`manifest::read`] returns
    /// `Ok(None)`) and must **not** error. The changed-site count is the
    /// [`STUB_CHANGED_SITES`] stub until differential hashing lands (S7/T14).
    fn scan_report(&self) -> anyhow::Result<ScanReport> {
        let source = std::fs::read_to_string(&self.file)
            .with_context(|| format!("failed to read source `{}`", self.file.display()))?;
        let total_sites = scanner::scan_source(&source)?.len();
        // Absence is a normal first run, not an error; the value is unused while
        // the changed count is stubbed (see STUB_CHANGED_SITES) but the read must
        // still surface genuine I/O / malformed-TOML failures.
        // TODO(S7/T14): compute real changed-site count via per-function hash diff.
        let _manifest = manifest::read(&manifest::sidecar_path(&self.file))?;
        Ok(ScanReport {
            file: self.file.display().to_string(),
            total_sites,
            changed_sites: STUB_CHANGED_SITES,
            warn_threshold: self.mutation_warning,
        })
    }

    /// Refreshes the sidecar manifest (`<file>.rs.m4r.toml`) from the target file.
    ///
    /// Reading the source from disk is infrastructure work done here; the parsed
    /// text is then handed to the pure [`manifest::function_hashes`] seam. Any
    /// I/O or parse failure maps onto [`ExitStatus::Error`] (C11: exit `1`).
    fn run_update_manifest(&self) -> ExitStatus {
        match self.update_manifest() {
            Ok(count) => {
                println!(
                    "mutate4rust: wrote manifest for {} ({count} functions).",
                    self.file.display()
                );
                ExitStatus::Success
            }
            Err(err) => {
                eprintln!("mutate4rust: {err:#}");
                ExitStatus::Error
            }
        }
    }

    /// Computes per-function hashes for the target file and writes the sidecar,
    /// returning the number of functions recorded.
    fn update_manifest(&self) -> anyhow::Result<usize> {
        let source = std::fs::read_to_string(&self.file)
            .with_context(|| format!("failed to read source `{}`", self.file.display()))?;
        let functions = manifest::function_hashes(&source)?;
        let last_run = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_secs();
        let count = functions.len();
        let manifest = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            hasher: INTERIM_HASHER_ID.to_owned(),
            last_run,
            functions,
        };
        manifest::write(&manifest::sidecar_path(&self.file), &manifest)?;
        Ok(count)
    }

    /// Runs the mutate loop for the target file and reports the buckets.
    ///
    /// Surviving mutants are **reported, not signalled** — a completed run is
    /// [`ExitStatus::Success`] (exit `0`) however many mutants survived (strict
    /// mutate4go parity, decision C11). Only a genuine failure — an unreadable or
    /// unparseable target, a red baseline suite, or an I/O error — maps onto
    /// [`ExitStatus::Error`] (exit `1`).
    fn run_mutation(&self) -> ExitStatus {
        println!("Mutating {}...", self.file.display());
        match self.mutate() {
            Ok(report) => {
                print!("{}", report.summary());
                ExitStatus::Success
            }
            Err(err) => {
                eprintln!("mutate4rust: {err:#}");
                ExitStatus::Error
            }
        }
    }

    /// Resolves the two caller-owned paths — the target `.rs` file and the crate
    /// root the test command runs in — and drives the pipeline.
    ///
    /// `--test-command` and `--timeout-factor` are still unwired (T17); the run
    /// uses [`runner::default_command`] and [`pipeline::DEFAULT_MUTANT_TIMEOUT`].
    fn mutate(&self) -> anyhow::Result<MutationReport> {
        let crate_root = crate_root_of(&self.file)?;
        pipeline::run(
            &self.file,
            &crate_root,
            &runner::default_command(),
            pipeline::DEFAULT_MUTANT_TIMEOUT,
        )
    }
}

/// The nearest ancestor directory of `target` containing a `Cargo.toml` — the
/// working directory the test command must run in so `cargo test` recompiles the
/// mutated file.
///
/// Deliberately separate from the target path itself: the guard binds to the
/// `.rs` file, the runner to the crate root.
fn crate_root_of(target: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::fs::canonicalize(target)
        .with_context(|| format!("failed to resolve target file `{}`", target.display()))?;
    absolute
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "no `Cargo.toml` found above `{}`; the target must live inside a cargo crate",
                target.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// clap's own invariant checker — catches conflicting/ill-formed arg definitions.
    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// A bare invocation binds the positional `<FILE>` and leaves flags at defaults.
    #[test]
    fn parses_minimal_invocation_with_defaults() {
        let cli = Cli::try_parse_from(["mutate4rust", "src/lib.rs"]).unwrap();

        assert_eq!(cli.file, PathBuf::from("src/lib.rs"));
        assert!(!cli.scan);
        assert!(!cli.update_manifest);
        assert_eq!(cli.lines, None);
        assert!(!cli.since_last_run);
        assert!(!cli.mutate_all);
        assert!(!cli.reuse_coverage);
        assert_eq!(cli.mutation_warning, DEFAULT_MUTATION_WARNING);
        assert_eq!(cli.timeout_factor, None);
        assert_eq!(cli.test_command, None);
        assert_eq!(cli.max_workers, None);
        assert!(!cli.verbose);
    }

    /// The mutation-warning default matches the feature file (50).
    #[test]
    fn mutation_warning_defaults_to_fifty() {
        let cli = Cli::try_parse_from(["mutate4rust", "a.rs"]).unwrap();
        assert_eq!(cli.mutation_warning, 50);
    }

    /// Every parity flag parses and maps onto its config field.
    #[test]
    fn parses_full_parity_flag_surface() {
        let cli = Cli::try_parse_from([
            "mutate4rust",
            "src/target.rs",
            "--scan",
            "--update-manifest",
            "--lines",
            "45,67",
            "--since-last-run",
            "--mutate-all",
            "--reuse-coverage",
            "--mutation-warning",
            "10",
            "--timeout-factor",
            "2.5",
            "--test-command",
            "cargo nextest run",
            "--max-workers",
            "4",
            "--verbose",
        ])
        .unwrap();

        assert_eq!(cli.file, PathBuf::from("src/target.rs"));
        assert!(cli.scan);
        assert!(cli.update_manifest);
        assert_eq!(cli.lines.as_deref(), Some("45,67"));
        assert!(cli.since_last_run);
        assert!(cli.mutate_all);
        assert!(cli.reuse_coverage);
        assert_eq!(cli.mutation_warning, 10);
        assert_eq!(cli.timeout_factor, Some(2.5));
        assert_eq!(cli.test_command.as_deref(), Some("cargo nextest run"));
        assert_eq!(cli.max_workers, Some(4));
        assert!(cli.verbose);
    }

    /// The positional `<FILE>` is required; omitting it is a usage error that,
    /// per strict mutate4go parity (C11), maps onto exit code `1` (not clap's
    /// default `2`).
    #[test]
    fn missing_file_is_a_usage_error() {
        let err = Cli::try_parse_from(["mutate4rust"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        assert_eq!(Cli::exit_status_for_parse_error(&err).raw_code(), 1);
    }

    /// `--help` is served by clap (not treated as a run) and exits `0`.
    #[test]
    fn help_flag_is_handled_by_clap() {
        let err = Cli::try_parse_from(["mutate4rust", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        assert_eq!(Cli::exit_status_for_parse_error(&err).raw_code(), 0);
    }

    /// `--version` is served by clap and reports the crate version, exiting `0`.
    #[test]
    fn version_flag_is_handled_by_clap() {
        let err = Cli::try_parse_from(["mutate4rust", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(Cli::exit_status_for_parse_error(&err).raw_code(), 0);
    }

    /// Deterministic snapshot of the *rendered* `--help` surface.
    ///
    /// The `DisplayHelp` error-kind check above proves clap serves `--help`, but
    /// not what it prints. This asserts against the actual rendered help text —
    /// the real regression surface — so any accidental rename or removal of a
    /// parity flag, the positional `<FILE>`, or the usage line is caught. Color
    /// is disabled and the bin name is pinned via `#[command(name = ...)]`, so
    /// the output is stable across environments (no color/locale/tty variance).
    #[test]
    fn help_output_snapshot_lists_every_parity_flag() {
        let help = Cli::command()
            .color(clap::ColorChoice::Never)
            .render_long_help()
            .to_string();

        // Usage line + required positional.
        assert!(
            help.contains("Usage: mutate4rust"),
            "missing usage line in rendered help:\n{help}"
        );
        assert!(
            help.contains("<FILE>"),
            "missing positional <FILE> in rendered help:\n{help}"
        );

        // Every parity flag (feature file C10) must appear in the rendered help.
        for flag in [
            "--scan",
            "--update-manifest",
            "--lines",
            "--since-last-run",
            "--mutate-all",
            "--reuse-coverage",
            "--mutation-warning",
            "--timeout-factor",
            "--test-command",
            "--max-workers",
            "--verbose",
            "--help",
            "--version",
        ] {
            assert!(
                help.contains(flag),
                "missing `{flag}` in rendered help:\n{help}"
            );
        }
    }

    /// Deterministic snapshot of the rendered `--version` surface: the exact
    /// `<name> <version>` line clap emits, pinned to the crate manifest version.
    #[test]
    fn version_output_snapshot_reports_crate_version() {
        let version = Cli::command().render_version().to_string();
        assert_eq!(
            version,
            format!("mutate4rust {}\n", env!("CARGO_PKG_VERSION"))
        );
    }

    /// The exit-code contract maps outcomes onto the documented process codes:
    /// success (incl. surviving mutants) is `0`, any error is `1` (C11 parity).
    #[test]
    fn exit_status_codes_match_contract() {
        assert_eq!(ExitStatus::Success.raw_code(), 0);
        assert_eq!(ExitStatus::Error.raw_code(), 1);
    }

    /// Mode resolution honours the deterministic precedence: `--scan` wins over
    /// `--update-manifest`, which wins over a plain mutation run.
    #[test]
    fn mode_resolution_follows_precedence() {
        let scan = Cli::try_parse_from(["mutate4rust", "a.rs", "--scan"]).unwrap();
        assert_eq!(Mode::resolve(&scan), Mode::Scan);

        let update = Cli::try_parse_from(["mutate4rust", "a.rs", "--update-manifest"]).unwrap();
        assert_eq!(Mode::resolve(&update), Mode::UpdateManifest);

        let mutate = Cli::try_parse_from(["mutate4rust", "a.rs"]).unwrap();
        assert_eq!(Mode::resolve(&mutate), Mode::Mutate);

        let both =
            Cli::try_parse_from(["mutate4rust", "a.rs", "--scan", "--update-manifest"]).unwrap();
        assert_eq!(Mode::resolve(&both), Mode::Scan);
    }

    /// `--update-manifest` end-to-end **through dispatch**: parses the flag and
    /// drives [`Cli::run`] (not the handler directly), proving `run()` routes
    /// `Mode::UpdateManifest`. Asserts the returned status is `Success` and that a
    /// sidecar is created beside the source that re-reads into a `Manifest` with a
    /// non-empty `functions` map. The temp dir is RAII-cleaned even on panic.
    #[test]
    fn update_manifest_writes_readable_sidecar() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let source_path = dir.path().join("target.rs");
        std::fs::write(
            &source_path,
            "fn a(x: i32, y: i32) -> i32 { x + y }\nfn b(x: i32, y: i32) -> i32 { x - y }\n",
        )
        .expect("seed source file");

        let cli = Cli::try_parse_from([
            "mutate4rust",
            source_path.to_str().unwrap(),
            "--update-manifest",
        ])
        .unwrap();
        let status = cli.run();

        assert_eq!(status, ExitStatus::Success);

        let sidecar = manifest::sidecar_path(&source_path);
        let recorded = manifest::read(&sidecar)
            .expect("sidecar should read")
            .expect("sidecar should exist beside the source");
        assert!(!recorded.functions.is_empty());
    }

    /// Seeds a temp `.rs` file with three in-scope arithmetic sites (`+ - *`) and
    /// returns the RAII temp dir plus the file path. The dir is dropped by the
    /// caller (cleaned even on panic).
    fn seed_three_site_source() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let source_path = dir.path().join("target.rs");
        std::fs::write(
            &source_path,
            "fn add(a: i32, b: i32) -> i32 { a + b }\n\
             fn sub(a: i32, b: i32) -> i32 { a - b }\n\
             fn mul(a: i32, b: i32) -> i32 { a * b }\n",
        )
        .expect("seed source file");
        (dir, source_path)
    }

    fn scan_cli(source_path: &Path, extra: &[&str]) -> Cli {
        let mut args = vec!["mutate4rust", source_path.to_str().unwrap(), "--scan"];
        args.extend_from_slice(extra);
        Cli::try_parse_from(args).unwrap()
    }

    /// The reported total equals `scan_source` for the same content — the scan
    /// report is a thin count over the pure scanner, and the changed count is the
    /// stub `0`.
    #[test]
    fn scan_report_total_matches_scan_source() {
        let (_dir, source_path) = seed_three_site_source();
        let source = std::fs::read_to_string(&source_path).unwrap();
        let expected = crate::scanner::scan_source(&source).unwrap().len();

        let report = scan_cli(&source_path, &[]).scan_report().expect("scan ok");

        assert_eq!(report.total_sites, expected);
        assert_eq!(report.total_sites, 3);
        assert_eq!(report.changed_sites, 0);
    }

    /// With **no sidecar present** the scan still succeeds and reports changed `0`
    /// — an absent manifest is the normal first run, not an error (C11). Driven
    /// through `run()` so `Mode::Scan` dispatch is exercised end-to-end.
    #[test]
    fn scan_without_sidecar_succeeds_and_is_not_an_error() {
        let (_dir, source_path) = seed_three_site_source();
        // No sidecar was written, so the manifest read returns Ok(None).
        assert_eq!(
            manifest::read(&manifest::sidecar_path(&source_path)).unwrap(),
            None,
        );

        let cli = scan_cli(&source_path, &[]);
        let report = cli.scan_report().expect("absent sidecar is not an error");
        assert_eq!(report.changed_sites, 0);

        // End-to-end through dispatch: Mode::Scan routes here and returns Success.
        assert_eq!(scan_cli(&source_path, &[]).run(), ExitStatus::Success);
    }

    /// With a sidecar present the scan still succeeds and changed remains the stub
    /// `0` (differential diffing isn't built yet — S7/T14).
    #[test]
    fn scan_with_sidecar_present_still_reports_stub_zero() {
        let (_dir, source_path) = seed_three_site_source();
        // Write a real sidecar beside the source via --update-manifest dispatch.
        let update = Cli::try_parse_from([
            "mutate4rust",
            source_path.to_str().unwrap(),
            "--update-manifest",
        ])
        .unwrap();
        assert_eq!(update.run(), ExitStatus::Success);
        assert!(
            manifest::read(&manifest::sidecar_path(&source_path))
                .unwrap()
                .is_some(),
            "sidecar should now exist",
        );

        let report = scan_cli(&source_path, &[]).scan_report().expect("scan ok");
        assert_eq!(report.changed_sites, 0);
        assert_eq!(scan_cli(&source_path, &[]).run(), ExitStatus::Success);
    }

    /// A missing target file is an I/O error → `ExitStatus::Error` (exit 1, C11).
    #[test]
    fn scan_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let missing = dir.path().join("does-not-exist.rs");
        let cli = scan_cli(&missing, &[]);

        assert!(cli.scan_report().is_err());
        assert_eq!(scan_cli(&missing, &[]).run(), ExitStatus::Error);
    }

    /// The over-threshold warning fires only when the total strictly exceeds the
    /// threshold; equal-to and below the threshold stay silent (pure logic).
    #[test]
    fn scan_report_warning_fires_only_above_threshold() {
        let report = |total, threshold| ScanReport {
            file: "x.rs".to_owned(),
            total_sites: total,
            changed_sites: 0,
            warn_threshold: threshold,
        };

        assert!(report(2, 1).warning().is_some(), "above threshold warns");
        assert!(report(1, 1).warning().is_none(), "at threshold is silent");
        assert!(
            report(0, 1).warning().is_none(),
            "below threshold is silent"
        );
    }

    /// A low `--mutation-warning` against a >1-site file triggers the advisory
    /// warning; the default (50) against the same file does not — exercised via
    /// the real handler path (`scan_report`), and both scans still succeed.
    #[test]
    fn scan_mutation_warning_threshold_wiring() {
        let (_dir, source_path) = seed_three_site_source();

        let low = scan_cli(&source_path, &["--mutation-warning", "1"])
            .scan_report()
            .expect("scan ok");
        assert!(low.warning().is_some(), "3 sites exceed threshold 1");
        assert_eq!(
            scan_cli(&source_path, &["--mutation-warning", "1"]).run(),
            ExitStatus::Success,
            "a warning is advisory, not an error",
        );

        let default = scan_cli(&source_path, &[]).scan_report().expect("scan ok");
        assert!(default.warning().is_none(), "3 sites are below default 50");
    }

    /// The summary is stable and greppable: the `Scanning`, `Mutation sites`, and
    /// `Changed sites` lines with the stub note, each newline-terminated.
    #[test]
    fn scan_report_summary_is_stable_and_greppable() {
        let report = ScanReport {
            file: "src/lib.rs".to_owned(),
            total_sites: 7,
            changed_sites: 0,
            warn_threshold: 50,
        };

        assert_eq!(
            report.summary(),
            "Scanning src/lib.rs...\n\
             Mutation sites: 7\n\
             Changed sites: 0 (differential not yet active; reported as 0 until S7)\n",
        );
    }

    /// The crate root is the nearest ancestor holding a `Cargo.toml` — resolved
    /// against this very crate, whose manifest dir is known at compile time.
    #[test]
    fn crate_root_resolves_to_the_enclosing_cargo_crate() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = crate_root_of(&manifest_dir.join("src").join("lib.rs")).expect("crate root");

        assert_eq!(
            root,
            std::fs::canonicalize(&manifest_dir).expect("canonicalize manifest dir"),
        );
    }

    /// A file with no `Cargo.toml` anywhere above it is a clear error, not a
    /// silent fallback to some unrelated directory.
    #[test]
    fn crate_root_errors_outside_any_crate() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let orphan = dir.path().join("target.rs");
        std::fs::write(&orphan, "fn f() {}\n").expect("seed source");

        assert!(crate_root_of(&orphan).is_err());
    }

    /// A mutation run against a missing target is an error → exit `1` (C11). The
    /// failure is raised while resolving paths, so no test command is ever spawned.
    #[test]
    fn mutation_run_on_a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let missing = dir.path().join("does-not-exist.rs");
        let cli = Cli::try_parse_from(["mutate4rust", missing.to_str().unwrap()]).unwrap();

        assert_eq!(Mode::resolve(&cli), Mode::Mutate);
        assert_eq!(cli.run(), ExitStatus::Error);
    }
}
