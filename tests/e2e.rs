//! The one end-to-end test: a real `mutate4rust` binary driven against a real
//! throwaway crate, exercising discover → coverage → mutate → test → report.
//!
//! Deliberately **one** test (golden rule #8). It is the only place that proves
//! the stages compose — `cargo-llvm-cov` really runs, its export really maps onto
//! the target's lines, and covered-only gating really changes which mutants are
//! executed. It is also what earns the CI `cargo-llvm-cov` install, which until
//! now proved only that the tool was present.
//!
//! Both phases run inside one `#[test]` because phase 2 (`--reuse-coverage`)
//! depends on the profile phase 1 wrote; splitting them would either duplicate a
//! ~30 s coverage build or couple two tests through shared mutable state.
//!
//! # Cost and flakiness
//!
//! This test compiles a two-function crate three-plus times (instrumented
//! coverage build, baseline, one mutant). It is slow but not timing-sensitive:
//! nothing asserts on durations, and the per-mutant timeout is the 300 s default.
//! It is excluded from the fast gate (`cargo test --lib --bins`) by living here.

use std::path::Path;
use std::process::Command;

/// The fixture's `src/lib.rs`.
///
/// Exactly two mutation sites survive discovery:
///
/// * line 5's `+` — **covered**, and `add`'s test pins the result, so mutating it
///   to `-` makes the suite red: **Killed**.
/// * line 10's `-` — `subtract` is never called by any test, so no coverage
///   region for line 10 is executed: **Uncovered**.
///
/// The `#[cfg(test)]` module's own literals are *not* discovered (T12's
/// deliberate divergence from upstream), which is why the expected totals below
/// are 1 + 1 and not "1 + 1 + however many literals the tests happen to use".
const FIXTURE_LIB: &str = "\
//! A two-function fixture crate.

/// Covered by the test below.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// Never exercised by any test.
pub fn subtract(a: i32, b: i32) -> i32 {
    a - b
}

#[cfg(test)]
mod tests {
    #[test]
    fn add_sums_its_arguments() {
        assert_eq!(super::add(2, 3), 5);
    }
}
";

const FIXTURE_MANIFEST: &str = "\
[package]
name = \"covfix\"
version = \"0.0.0\"
edition = \"2021\"

[dependencies]
";

/// The upstream-parity notice printed on the reuse path.
const REUSE_NOTICE: &str =
    "Reusing existing coverage; covered/uncovered classification may be stale.";

/// Writes the fixture crate into `root`.
fn seed_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create fixture src");
    std::fs::write(root.join("Cargo.toml"), FIXTURE_MANIFEST).expect("write fixture manifest");
    std::fs::write(root.join("src/lib.rs"), FIXTURE_LIB).expect("write fixture lib");
}

/// Runs the real binary against the fixture and returns `(stdout, stderr, code)`.
///
/// The ambient cargo environment is scrubbed: this process is itself running
/// under `cargo test` (and possibly under `cargo llvm-cov`), and inheriting
/// `RUSTFLAGS`, `CARGO_TARGET_DIR`, `LLVM_PROFILE_FILE` or the `CARGO_LLVM_COV*`
/// markers would make the nested instrumented build either reuse the outer
/// target dir or refuse to re-instrument.
fn run_binary(root: &Path, extra: &[&str]) -> (String, String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mutate4rust"));
    command.arg("src/lib.rs").args(extra).current_dir(root);
    for key in [
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "LLVM_PROFILE_FILE",
        "CARGO_LLVM_COV",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_SHOW_ENV",
    ] {
        command.env_remove(key);
    }

    let output = command.output().expect("spawn mutate4rust");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// Asserts the report says exactly what the fixture's coverage implies.
fn assert_expected_report(stdout: &str, stderr: &str, code: i32) {
    assert_eq!(code, 0, "C11: a completed run exits 0\n{stdout}\n{stderr}");
    assert!(
        stdout.contains("Killed:    1"),
        "the covered `+` must be killed:\n{stdout}\n{stderr}",
    );
    assert!(
        stdout.contains("Survived:  0"),
        "nothing should survive:\n{stdout}\n{stderr}",
    );
    assert!(
        stdout.contains("Uncovered: 1"),
        "the uncovered `-` must be reported, not mutated:\n{stdout}\n{stderr}",
    );
    assert!(
        stdout.contains("Score:     100.0%"),
        "1 killed / (1 killed + 0 survived):\n{stdout}",
    );
    assert!(
        stdout.contains("src/lib.rs:10") || stdout.contains("src\\lib.rs:10"),
        "the uncovered site must be listed by location:\n{stdout}",
    );
    // Exactly one uncovered entry — a site inside `#[cfg(test)] mod tests` would
    // be covered-by-construction and so could only ever show up as a *mutated*
    // site, which `Killed: 1` + `Survived: 0` already excludes. (The listing is
    // counted here rather than grepped for a line number because the mutant's own
    // test output — which quotes `src/lib.rs:17` — is inherited into this stdout.)
    let uncovered_entries = stdout
        .lines()
        .skip_while(|line| !line.starts_with("Uncovered sites:"))
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .count();
    assert_eq!(uncovered_entries, 1, "one listed uncovered site:\n{stdout}");
}

#[test]
fn a_real_run_discovers_covers_mutates_and_reports() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    seed_fixture(root);

    // Phase 1: generate coverage from scratch.
    let (stdout, stderr, code) = run_binary(root, &[]);
    assert!(
        !stdout.contains(REUSE_NOTICE),
        "a fresh run must not claim to reuse anything:\n{stdout}",
    );
    assert_expected_report(&stdout, &stderr, code);
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).expect("read target"),
        FIXTURE_LIB,
        "the target must be pristine after the run",
    );

    // Phase 2: reuse the profile phase 1 left behind. The notice is upstream
    // parity and must appear verbatim on stdout.
    let (reused_stdout, reused_stderr, reused_code) = run_binary(root, &["--reuse-coverage"]);
    assert!(
        reused_stdout.contains(REUSE_NOTICE),
        "the reuse notice must be printed verbatim:\n{reused_stdout}",
    );
    assert_expected_report(&reused_stdout, &reused_stderr, reused_code);
}

/// `--reuse-coverage` without a profile is an error (A6), not a silent
/// everything-uncovered run. Cheap — it never builds anything.
#[test]
fn reuse_without_a_profile_exits_one() {
    let dir = tempfile::tempdir().expect("create temp dir");
    seed_fixture(dir.path());

    let (stdout, stderr, code) = run_binary(dir.path(), &["--reuse-coverage"]);

    assert_eq!(code, 1, "C11: any error exits 1\n{stdout}\n{stderr}");
    assert!(
        !stdout.contains("Killed:"),
        "no report may be printed:\n{stdout}",
    );
    assert!(
        stderr.contains("coverage"),
        "the error must name the problem:\n{stderr}",
    );
}
