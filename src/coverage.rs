//! `cargo-llvm-cov` invocation and JSON-export parsing (**Infrastructure**,
//! design.md → the only layer touching process/fs).
//!
//! Stable Rust ships no built-in line coverage, so `cargo-llvm-cov` is the
//! backend (conflict C2). This module is the boundary: it spawns the tool, reads
//! its profile, and parses the export into the pure
//! [`CoverageMap`](crate::coverage_map::CoverageMap) that the pipeline queries
//! inward. Nothing in Core ever sees this module or the export format.
//!
//! # Why the JSON export, not the text report
//!
//! `cargo llvm-cov --json` emits LLVM's `llvm.coverage.json.export` document: a
//! versioned, machine-oriented structure carrying **explicit region tuples**
//! `[line_start, col_start, line_end, col_end, count, file_id, …]`. The human
//! text report only prints per-file percentages, and the `--lcov` output has
//! already collapsed regions to lines using llvm-cov's own rule — which would
//! hide, not solve, the C8/R5 mapping we are accountable for. The JSON export is
//! the only format that hands us the raw region ranges, so the mapping rule is
//! ours, documented, and unit-testable.
//!
//! # Artifact location (A2)
//!
//! The profile is written under the crate's own `target/` directory — llvm-cov's
//! convention, and the direct port of upstream mutate4go's
//! `target/coverage/coverage.out`. No new location is invented, and the file is
//! produced by the tool, never hand-edited.
//!
//! # Missing tooling (R8)
//!
//! A missing backend **fails loudly** with the install commands, and never
//! degrades into "mutate everything" — silently treating an absent profile as
//! "nothing is covered" would invert the gate. The two failure modes are kept
//! distinct in the message: *not installed* ([`ensure_backend_installed`]) versus
//! *installed but the run failed* ([`generate`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::coverage_map::{CoverageMap, Region};

/// Coverage profile location, relative to the crate root (A2).
const PROFILE_RELATIVE_PATH: &str = "target/coverage/coverage.json";

/// Actionable install hint for an absent coverage backend (R8).
const MISSING_BACKEND_HINT: &str = "the coverage backend is not installed \
     (`cargo llvm-cov --version` could not be run)\n\
     install it and re-run:\n    \
     cargo install cargo-llvm-cov\n    \
     rustup component add llvm-tools-preview";

/// Number of leading fields every region tuple in the export carries.
const REGION_FIELDS: usize = 8;
/// Index of `line_start` within a region tuple.
const REGION_START_LINE: usize = 0;
/// Index of `line_end` within a region tuple.
const REGION_END_LINE: usize = 2;
/// Index of the execution count within a region tuple.
const REGION_COUNT: usize = 4;
/// Index of the `file_id` (into the function's `filenames`) within a region tuple.
const REGION_FILE_ID: usize = 5;

/// Where the coverage profile for `crate_root` lives.
#[must_use]
pub fn profile_path(crate_root: &Path) -> PathBuf {
    crate_root.join(PROFILE_RELATIVE_PATH)
}

/// Fails with an install hint unless the coverage backend can be executed (R8).
///
/// # Errors
///
/// Returns an error naming both install commands when `cargo llvm-cov --version`
/// cannot be spawned or exits non-zero.
pub fn ensure_backend_installed() -> Result<()> {
    check_backend(&backend_probe())
}

/// The command used to prove the backend is present.
fn backend_probe() -> Vec<String> {
    vec![
        "cargo".to_owned(),
        "llvm-cov".to_owned(),
        "--version".to_owned(),
    ]
}

/// Runs `probe` purely for its exit status, mapping any failure onto the install
/// hint. Parameterised on the command so the missing-tool path is testable on a
/// machine where the real backend *is* installed.
fn check_backend(probe: &[String]) -> Result<()> {
    let (program, args) = probe
        .split_first()
        .context("coverage probe command is empty")?;
    let status = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        _ => bail!(MISSING_BACKEND_HINT),
    }
}

/// Runs a fresh coverage pass over `crate_root` and returns line coverage for
/// `target` (A6 default path: coverage first, then mutate only covered sites).
///
/// # Errors
///
/// Returns an error if the backend is not installed, if the coverage run itself
/// fails (a distinct, non-install message), or if the resulting profile cannot be
/// read or parsed.
pub fn generate(crate_root: &Path, target: &Path) -> Result<CoverageMap> {
    ensure_backend_installed()?;

    let profile = profile_path(crate_root);
    let directory = profile
        .parent()
        .context("coverage profile path has no parent directory")?;
    std::fs::create_dir_all(directory).with_context(|| {
        format!(
            "failed to create the coverage directory `{}`",
            directory.display()
        )
    })?;

    run_coverage(&coverage_command(&profile), crate_root)?;

    load(&profile, target)
}

/// The command that writes the coverage profile to `profile`.
fn coverage_command(profile: &Path) -> Vec<OsString> {
    vec![
        "cargo".into(),
        "llvm-cov".into(),
        "--json".into(),
        "--output-path".into(),
        profile.into(),
    ]
}

/// Runs `command` inside `crate_root`, mapping a non-zero exit onto the
/// *distinct* "ran but failed" error (R8) — never the install hint. Parameterised
/// on the command, exactly like [`check_backend`], so the failing-run path is
/// testable without a real coverage pass.
fn run_coverage(command: &[OsString], crate_root: &Path) -> Result<()> {
    let (program, args) = command.split_first().context("coverage command is empty")?;
    let rendered = command
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let output = Command::new(program)
        .args(args)
        .current_dir(crate_root)
        .output()
        .with_context(|| format!("failed to spawn `{rendered}`"))?;

    if !output.status.success() {
        // Deliberately NOT the install hint: the tool is present and ran — the
        // run failed, most often because the suite itself is red.
        bail!(
            "`{rendered}` ran but failed ({}) in `{}`\n{}",
            output.status,
            crate_root.display(),
            String::from_utf8_lossy(&output.stderr).trim_end(),
        );
    }

    Ok(())
}

/// Loads an **existing** profile for `--reuse-coverage` (A6).
///
/// Absence is a hard error, matching upstream mutate4go's `ensureCoverage`, which
/// errors with "`--reuse-coverage` was requested, but … does not exist" rather
/// than regenerating or treating everything as uncovered.
///
/// # Errors
///
/// Returns an error if no profile exists at [`profile_path`], or if the profile
/// cannot be read or parsed.
pub fn reuse(crate_root: &Path, target: &Path) -> Result<CoverageMap> {
    let profile = profile_path(crate_root);
    // `is_file` folds "absent" and "unreadable metadata" into one answer; both
    // mean the profile cannot be reused, so the remedy printed below is the same.
    if !profile.is_file() {
        bail!(
            "--reuse-coverage was requested, but `{}` does not exist\n\
             run without --reuse-coverage once to generate coverage",
            profile.display()
        );
    }
    load(&profile, target)
}

/// Reads a profile from disk and parses it for `target`.
fn load(profile: &Path, target: &Path) -> Result<CoverageMap> {
    let json = std::fs::read_to_string(profile)
        .with_context(|| format!("failed to read coverage profile `{}`", profile.display()))?;
    parse_export(&json, target)
        .with_context(|| format!("failed to parse coverage profile `{}`", profile.display()))
}

/// Parses an `llvm.coverage.json.export` document into line coverage for
/// `target`, discarding every region belonging to another file.
///
/// # Errors
///
/// Returns an error if the document is not valid JSON in the expected shape, if a
/// region tuple is shorter than [`REGION_FIELDS`], or if a region names a
/// `file_id` the function does not declare.
pub fn parse_export(json: &str, target: &Path) -> Result<CoverageMap> {
    let export: Export =
        serde_json::from_str(json).context("failed to parse the cargo-llvm-cov JSON export")?;

    let mut regions = Vec::new();
    for data in &export.data {
        for function in &data.functions {
            for raw in &function.regions {
                if raw.len() < REGION_FIELDS {
                    bail!(
                        "coverage region has {} fields, expected at least {REGION_FIELDS}",
                        raw.len()
                    );
                }
                let file_id = usize::try_from(raw[REGION_FILE_ID])
                    .context("coverage region file id does not fit in a pointer-sized integer")?;
                let filename = function
                    .filenames
                    .get(file_id)
                    .with_context(|| format!("coverage region names unknown file id {file_id}"))?;
                if !matches_target(filename, target) {
                    continue;
                }
                regions.push(Region {
                    start_line: line_number(raw[REGION_START_LINE])?,
                    end_line: line_number(raw[REGION_END_LINE])?,
                    count: raw[REGION_COUNT],
                });
            }
        }
    }

    Ok(CoverageMap::new(regions))
}

/// Narrows an export line number to a `usize`.
fn line_number(raw: u64) -> Result<usize> {
    usize::try_from(raw)
        .context("coverage region line number does not fit in a pointer-sized integer")
}

/// Whether the export's `candidate` filename denotes `target`.
///
/// llvm-cov emits absolute, platform-native paths while the CLI's target may be
/// relative, so the target's path segments must be a **suffix** of the
/// candidate's — the same normalise-then-suffix-match rule upstream mutate4go
/// applies to Go coverage profiles. Separators are normalised, so a Windows
/// `C:\work\covfix\src\lib.rs` matches a `src/lib.rs` target.
fn matches_target(candidate: &str, target: &Path) -> bool {
    let candidate = normalise(candidate);
    let target = normalise(&target.display().to_string());
    let candidate = path_segments(&candidate);
    let target = path_segments(&target);
    if target.is_empty() || target.len() > candidate.len() {
        return false;
    }
    let offset = candidate.len() - target.len();
    candidate[offset..] == target[..]
}

/// Normalises separators and drops a leading `./` so paths from either platform
/// compare on equal terms.
fn normalise(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_owned()
}

/// Splits a normalised path into its non-empty segments.
fn path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty()).collect()
}

/// Top level of the llvm-cov JSON export. Unknown fields (`type`, `version`,
/// `files`, summaries…) are ignored — we consume only the region tuples.
#[derive(Debug, Deserialize)]
struct Export {
    data: Vec<ExportData>,
}

/// One export unit; `functions` is where the line-ranged regions live.
#[derive(Debug, Deserialize)]
struct ExportData {
    #[serde(default)]
    functions: Vec<ExportFunction>,
}

/// A single function's coverage: the files it spans plus its region tuples.
#[derive(Debug, Deserialize)]
struct ExportFunction {
    filenames: Vec<String>,
    regions: Vec<Vec<u64>>,
}

#[cfg(test)]
mod tests {
    use super::{
        MISSING_BACKEND_HINT, backend_probe, check_backend, coverage_command, matches_target,
        parse_export, profile_path, reuse, run_coverage,
    };
    use crate::scanner;
    use crate::site::{Operator, Site};
    use std::ffi::OsString;
    use std::path::Path;

    /// A `cargo llvm-cov --json` capture (cargo-llvm-cov 0.9.0, export version
    /// 3.1.0) taken against [`FIXTURE_SOURCE`], **reduced** — not verbatim: the
    /// unconsumed bulk (`data[].totals` and each `files[]` entry's `branches`,
    /// `expansions`, `segments`, `summary`) was stripped and each region tuple
    /// re-indented onto one line. Every region tuple is otherwise exactly as the
    /// tool emitted it — including the multi-line loop-body region
    /// `[3, 25, 5, 6, 3, …]` that makes line 4 covered without any region
    /// starting on it.
    const FIXTURE_EXPORT: &str = include_str!("testdata/llvm-cov-export.json");

    /// The source [`FIXTURE_EXPORT`] was captured from, byte-identical to the
    /// fixture crate's `src/lib.rs`, so line numbers here are the tool's own.
    ///
    /// Line 4 is a **covered** statement line carrying exactly four sites (`+=`,
    /// `*`, `+`, and the constant `1`); line 11 is an **uncovered** statement line
    /// carrying exactly two (`+=`, `*`).
    const FIXTURE_SOURCE: &str = r"pub fn accumulate(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        total += value * 2 + 1;
    }
    total
}

pub fn never_called(a: i32, b: i32) -> i32 {
    let mut acc = a;
    acc += b * 2;
    acc
}

#[cfg(test)]
mod tests {
    #[test]
    fn accumulates() {
        assert_eq!(super::accumulate(&[1, 2, 3]), 15);
    }
}
";

    /// The fixture path as llvm-cov spells it, relative to the fixture crate.
    fn fixture_target() -> &'static Path {
        Path::new("src/lib.rs")
    }

    /// Every site the scanner finds on `line` of [`FIXTURE_SOURCE`].
    fn sites_on_line(line: usize) -> Vec<Site> {
        scanner::scan_source(FIXTURE_SOURCE)
            .expect("fixture parses")
            .into_iter()
            .filter(|site| site.line == line)
            .collect()
    }

    /// A one-line script run through the platform's own shell, so both process
    /// failure modes can be provoked without touching the real backend.
    fn shell_command(windows: &str, unix: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".to_owned(), "/C".to_owned(), windows.to_owned()]
        } else {
            vec!["sh".to_owned(), "-c".to_owned(), unix.to_owned()]
        }
    }

    /// [`shell_command`] shaped for the coverage-run seam.
    fn os_shell_command(windows: &str, unix: &str) -> Vec<OsString> {
        shell_command(windows, unix)
            .into_iter()
            .map(OsString::from)
            .collect()
    }

    #[test]
    fn profile_lives_under_the_crate_target_directory() {
        // A2: llvm-cov's own convention, no invented location.
        let profile = profile_path(Path::new("/crate"));
        let rendered = profile.display().to_string().replace('\\', "/");
        assert!(
            rendered.ends_with("target/coverage/coverage.json"),
            "{rendered}"
        );
    }

    #[test]
    fn the_backend_probe_invokes_cargo_llvm_cov() {
        assert_eq!(backend_probe(), vec!["cargo", "llvm-cov", "--version"]);
    }

    #[test]
    fn an_unavailable_backend_fails_with_both_install_commands() {
        // R8: proven without uninstalling anything — the probe command is a seam,
        // so an unresolvable program stands in for an absent backend.
        let err = check_backend(&["mutate4rust-no-such-coverage-backend".to_owned()])
            .expect_err("an unresolvable backend must fail loudly");
        let message = format!("{err:#}");

        assert!(message.contains("not installed"), "{message}");
        assert!(
            message.contains("cargo install cargo-llvm-cov"),
            "{message}"
        );
        assert!(
            message.contains("rustup component add llvm-tools-preview"),
            "{message}"
        );
    }

    #[test]
    fn a_backend_that_runs_but_exits_non_zero_is_also_treated_as_unavailable() {
        let err = check_backend(&shell_command("exit 1", "exit 1"))
            .expect_err("a non-zero probe must fail loudly");
        let message = format!("{err:#}");

        assert!(message.contains("not installed"), "{message}");
        assert!(
            message.contains("cargo install cargo-llvm-cov"),
            "{message}"
        );
        assert!(
            message.contains("rustup component add llvm-tools-preview"),
            "{message}"
        );
    }

    #[test]
    fn the_coverage_command_writes_the_json_export_to_the_profile() {
        let command = coverage_command(Path::new("/crate/target/coverage/coverage.json"));
        assert_eq!(
            command,
            vec![
                OsString::from("cargo"),
                OsString::from("llvm-cov"),
                OsString::from("--json"),
                OsString::from("--output-path"),
                OsString::from("/crate/target/coverage/coverage.json"),
            ]
        );
    }

    #[test]
    fn a_coverage_run_that_fails_reports_the_status_and_the_stderr() {
        // R8's *other* failure mode, exercised for real: the tool ran, so the
        // message must say so, carry the process status and the tool's stderr,
        // and must NOT be misclassified as "not installed".
        //
        // The failing behaviour lives in a *script file*, invoked by a
        // digit-free relative name from the run directory, so the rendered
        // command carries neither the marker token nor the exit status: the
        // full-equality assertion below can only hold if `run_coverage` really
        // interpolates `output.status` and `output.stderr`, in that order.
        let dir = tempfile::tempdir().expect("create temp dir");
        let (script, body) = if cfg!(windows) {
            (
                "covfail.bat",
                "@echo off\r\necho covfixboomtoken 1>&2\r\nexit 7\r\n",
            )
        } else {
            ("covfail.sh", "echo covfixboomtoken 1>&2\nexit 7\n")
        };
        std::fs::write(dir.path().join(script), body).expect("write script");
        // Interpreter + bare script name only — no `-c` inline text.
        let command: Vec<OsString> = if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), format!(".\\{script}").into()]
        } else {
            vec!["sh".into(), script.into()]
        };
        let rendered = command
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");

        // A *real* run of the very same script supplies the two process-derived
        // components — the platform's `ExitStatus` rendering ("exit code: 7" on
        // Windows, "exit status: 7" on Unix) and the raw stderr bytes — so the
        // expectation tracks the platform instead of drifting literals.
        let probe = std::process::Command::new(&command[0])
            .args(&command[1..])
            .current_dir(dir.path())
            .output()
            .expect("run the failing script once to learn its status rendering");
        assert_eq!(probe.status.code(), Some(7), "the script must exit 7");

        let err = run_coverage(&command, dir.path()).expect_err("a failing run must error");
        let message = format!("{err:#}");

        // Full-equality against the message rebuilt from the same four
        // components production interpolates. This proves status *and* stderr
        // propagation positionally — including their order — and cannot be
        // satisfied, or broken, by an incidental substring from the environment.
        let expected = format!(
            "`{rendered}` ran but failed ({}) in `{}`\n{}",
            probe.status,
            dir.path().display(),
            String::from_utf8_lossy(&probe.stderr).trim_end(),
        );
        assert_eq!(message, expected);

        assert!(
            !message.contains("not installed") && !message.contains("cargo install cargo-llvm-cov"),
            "a failed run must not be sold as a missing install: {message}"
        );
    }

    #[test]
    fn a_coverage_run_whose_command_cannot_be_spawned_names_the_program() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let command = vec![OsString::from("mutate4rust-no-such-coverage-backend")];
        let err = run_coverage(&command, dir.path()).expect_err("an unspawnable run must error");
        let message = format!("{err:#}");

        assert!(
            message.contains("failed to spawn `mutate4rust-no-such-coverage-backend`"),
            "{message}"
        );
        assert!(!message.contains("ran but failed"), "{message}");
    }

    #[test]
    fn a_successful_coverage_run_is_not_an_error() {
        let dir = tempfile::tempdir().expect("create temp dir");
        assert!(run_coverage(&os_shell_command("exit 0", "exit 0"), dir.path()).is_ok());
    }

    #[test]
    fn the_install_hint_never_claims_the_tool_merely_failed() {
        // The two R8 failure modes must stay distinguishable: this is the
        // "not installed" text; `generate`'s non-zero-exit path says "ran but
        // failed" instead.
        assert!(!MISSING_BACKEND_HINT.contains("ran but failed"));
    }

    #[test]
    fn reuse_without_a_profile_errors_and_names_the_remedy() {
        // A6, matching upstream mutate4go `ensureCoverage`: an absent profile
        // under --reuse-coverage is an error, never a silent empty profile.
        let dir = tempfile::tempdir().expect("create temp dir");
        let err = reuse(dir.path(), fixture_target()).expect_err("absent profile must error");
        let message = format!("{err:#}");

        assert!(message.contains("--reuse-coverage"), "{message}");
        assert!(message.contains("does not exist"), "{message}");
        assert!(
            message.contains("target") && message.contains("coverage.json"),
            "the message must name the profile it looked for: {message}"
        );
    }

    #[test]
    fn parsing_a_real_export_maps_regions_onto_lines() {
        let map = parse_export(FIXTURE_EXPORT, fixture_target()).expect("fixture export parses");

        assert!(!map.is_empty());
        assert!(map.is_line_covered(1), "covered fn signature");
        assert!(map.is_line_covered(6), "covered fn body");
        assert!(!map.is_line_covered(9), "uncovered fn signature");
        assert!(!map.is_line_covered(12), "uncovered fn body");
    }

    #[test]
    fn a_covered_compound_assignment_statement_line_is_covered() {
        // The T10 carry-forward, and R5's exact failure mode: line 4 sits INSIDE
        // the loop-body region `[3, 25] .. [5, 6]` and no region starts on it. A
        // naive start-line mapping would call it uncovered and skip the site.
        let map = parse_export(FIXTURE_EXPORT, fixture_target()).expect("fixture export parses");
        let sites = sites_on_line(4);

        assert!(
            sites
                .iter()
                .any(|site| site.operator == Operator::AddAssign),
            "line 4 must hold the `+=` site: {sites:?}"
        );
        assert!(
            map.is_line_covered(4),
            "`total += value * 2 + 1;` is covered"
        );
    }

    #[test]
    fn every_site_on_a_multi_site_line_resolves_consistently() {
        // `site.line` is the whole coverage key, so all FOUR sites on the covered
        // line 4 (`+=`, `*`, `+`, `1`) must agree, and likewise BOTH sites on the
        // uncovered line 11 (`+=`, `*`). Counts are pinned so the proof cannot
        // silently degrade into a single-site line.
        let map = parse_export(FIXTURE_EXPORT, fixture_target()).expect("fixture export parses");

        let covered = sites_on_line(4);
        assert_eq!(covered.len(), 4, "line 4 must hold four sites: {covered:?}");
        assert!(
            covered.iter().all(|site| map.is_line_covered(site.line)),
            "every site on a covered line must read covered: {covered:?}"
        );

        let uncovered = sites_on_line(11);
        assert_eq!(
            uncovered.len(),
            2,
            "line 11 must hold two sites: {uncovered:?}"
        );
        assert!(
            uncovered.iter().all(|site| !map.is_line_covered(site.line)),
            "every site on an uncovered line must read uncovered: {uncovered:?}"
        );
    }

    #[test]
    fn regions_belonging_to_another_file_are_discarded() {
        let map =
            parse_export(FIXTURE_EXPORT, Path::new("src/other.rs")).expect("fixture export parses");
        assert!(map.is_empty(), "no region belongs to `src/other.rs`");
    }

    #[test]
    fn an_absolute_target_matches_the_export_filename() {
        let map = parse_export(FIXTURE_EXPORT, Path::new(r"C:\work\covfix\src\lib.rs"))
            .expect("fixture export parses");
        assert!(map.is_line_covered(4));
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(parse_export("{ not json", fixture_target()).is_err());
    }

    #[test]
    fn a_short_region_tuple_is_an_error() {
        // A format change must fail loudly rather than silently drop regions and
        // report a covered file as uncovered.
        let json = r#"{"data":[{"functions":[{"filenames":["src/lib.rs"],"regions":[[1,1,2]]}]}]}"#;
        let err = parse_export(json, fixture_target()).expect_err("short tuple must error");
        assert!(format!("{err:#}").contains("expected at least"), "{err:#}");
    }

    #[test]
    fn a_region_naming_an_unknown_file_id_is_an_error() {
        let json = r#"{"data":[{"functions":[{"filenames":["src/lib.rs"],"regions":[[1,1,2,1,1,7,0,0]]}]}]}"#;
        let err = parse_export(json, fixture_target()).expect_err("bad file id must error");
        assert!(format!("{err:#}").contains("unknown file id"), "{err:#}");
    }

    #[test]
    fn an_export_without_functions_yields_an_empty_map() {
        let json = r#"{"data":[{"files":[]}]}"#;
        let map = parse_export(json, fixture_target()).expect("parses");
        assert!(map.is_empty());
    }

    #[test]
    fn path_matching_is_suffix_based_and_separator_agnostic() {
        assert!(matches_target(
            r"C:\work\covfix\src\lib.rs",
            Path::new("src/lib.rs")
        ));
        assert!(matches_target(
            "/home/dev/covfix/src/lib.rs",
            Path::new("src/lib.rs")
        ));
        assert!(matches_target(
            "/home/dev/covfix/src/lib.rs",
            Path::new("./src/lib.rs")
        ));
        // Partial segment names must not match — `lib.rs` is not `mylib.rs`.
        assert!(!matches_target(
            "/home/dev/covfix/src/mylib.rs",
            Path::new("src/lib.rs")
        ));
        assert!(!matches_target(
            "src/lib.rs",
            Path::new("covfix/src/lib.rs")
        ));
    }
}
