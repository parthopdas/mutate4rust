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

use clap::Parser;

/// Default mutation-count warning threshold (feature file `--scan`/T6 note).
const DEFAULT_MUTATION_WARNING: usize = 50;

/// Exit-code contract for `mutate4rust`, used as a CI guardrail.
///
/// The codes are stable and documented so pipelines can branch on them:
///
/// - `0` — **success**: the run (or scan) completed with no surviving mutants
///   (every covered mutant was killed, or there was nothing to report).
/// - `1` — **survivors**: mutation testing completed but at least one mutant
///   survived; the guardrail fails so CI can block the change.
/// - `2` — **usage**: invalid command-line usage. Emitted by `clap` itself when
///   argument parsing fails (this variant is not produced by our own code).
/// - `3` — **error**: an operational failure (file I/O, `syn` parse, coverage
///   tooling, or the test runner) prevented the run from completing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// `Survivors`/`Error` are part of the documented exit-code contract but are only
// produced from later slices; the S1 stubs return `Success`. `allow` (rather than
// `expect`) is required because the test build *does* construct them, so the
// expectation would be unfulfilled there.
#[allow(dead_code)]
enum ExitStatus {
    /// No surviving mutants — the guardrail passes.
    Success,
    /// At least one mutant survived — the guardrail fails.
    Survivors,
    /// An operational error prevented completion.
    Error,
}

impl ExitStatus {
    /// The raw exit code defined by the contract above.
    fn raw_code(self) -> u8 {
        match self {
            ExitStatus::Success => 0,
            ExitStatus::Survivors => 1,
            ExitStatus::Error => 3,
        }
    }

    /// Maps the outcome onto the documented process exit code.
    fn code(self) -> ExitCode {
        ExitCode::from(self.raw_code())
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
    /// `clap` handles `--help`/`--version` and usage errors internally (exiting
    /// with code `2` on bad usage); everything else flows through [`Self::run`].
    #[must_use]
    pub fn main() -> ExitCode {
        Self::parse().run()
    }

    /// Dispatches the parsed options to the appropriate stub handler.
    #[must_use]
    fn run(self) -> ExitCode {
        let status = if self.scan {
            self.run_scan()
        } else {
            self.run_mutation()
        };
        status.code()
    }

    /// Stub for `--scan` mode (site counting lands in S2 / T6).
    fn run_scan(&self) -> ExitStatus {
        println!(
            "mutate4rust: --scan for {} is not yet implemented (arrives in S2).",
            self.file.display()
        );
        ExitStatus::Success
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

    /// The positional `<FILE>` is required; omitting it is a usage error.
    #[test]
    fn missing_file_is_a_usage_error() {
        let err = Cli::try_parse_from(["mutate4rust"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    /// `--help` is served by clap (not treated as a run).
    #[test]
    fn help_flag_is_handled_by_clap() {
        let err = Cli::try_parse_from(["mutate4rust", "--help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    /// `--version` is served by clap and reports the crate version.
    #[test]
    fn version_flag_is_handled_by_clap() {
        let err = Cli::try_parse_from(["mutate4rust", "--version"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
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

    /// The exit-code contract maps outcomes onto the documented process codes.
    #[test]
    fn exit_status_codes_match_contract() {
        assert_eq!(ExitStatus::Success.raw_code(), 0);
        assert_eq!(ExitStatus::Survivors.raw_code(), 1);
        assert_eq!(ExitStatus::Error.raw_code(), 3);
    }
}
