//! Command-line adapter for `mutate4rust`.
//!
//! This module is an **infrastructure/adapter** concern living at the outer edge
//! of the crate (per the T1 design review): it parses process arguments with
//! `clap` and dispatches into the engine. The pure core (mutation-site model,
//! operator mappings) must never depend on this module — the dependency arrow
//! points inward, from adapters toward the core.
//!
//! During the S1 bootstrap the flag surface below is **parse-only**: every option
//! mirrors mutate4go's CLI for 1:1 muscle-memory parity (feature file C10), but the
//! handlers are stubs. Scan, coverage, mutation, manifest, and test-running logic
//! land in later slices.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::Parser;

use crate::manifest::{self, CURRENT_SCHEMA_VERSION, INTERIM_HASHER_ID, Manifest};

/// Default mutation-count warning threshold (feature file `--scan`/T6 note).
const DEFAULT_MUTATION_WARNING: usize = 50;

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

    /// Stub for `--scan` mode (site counting lands in S2 / T6).
    fn run_scan(&self) -> ExitStatus {
        println!(
            "mutate4rust: --scan for {} is not yet implemented (arrives in S2).",
            self.file.display()
        );
        ExitStatus::Success
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

    /// Stub for a normal mutation run (the mutate loop lands in S3).
    fn run_mutation(&self) -> ExitStatus {
        println!(
            "mutate4rust: mutation run for {} is not yet implemented (arrives in S3).",
            self.file.display()
        );
        ExitStatus::Success
    }
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
}
