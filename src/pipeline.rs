//! The mutate loop (Application layer): baseline check → per-site
//! apply/test/classify/restore → report.
//!
//! This module orchestrates the existing seams without fusing them — [`splice`]
//! stays pure, [`RestoreGuard`] owns the fs round-trip, and [`TestRunner`] owns
//! the subprocess. It takes **domain primitives** (`&Path`, `&[String]`,
//! [`Duration`]), never the clap `Cli`.
//!
//! # Two distinct caller-owned paths
//!
//! `target` is the `.rs` file being mutated (what [`RestoreGuard`] binds to);
//! `crate_root` is the directory the test command runs in, so `cargo test`
//! recompiles the mutated file. They are deliberately separate parameters — the
//! caller decides both (in a future multi-worker mode, both must point at the
//! worker's isolated copy, never the canonical source).
//!
//! # Zero mutation accumulation
//!
//! Every mutant is spliced from `guard.original()` — the pristine in-memory copy
//! read once at the start — so mutations can never stack across sites. The guard
//! is restored explicitly at the end of every iteration (and by `Drop` on any
//! earlier bail-out).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::apply::{RestoreGuard, splice};
use crate::operators;
use crate::report::MutationReport;
use crate::runner::TestRunner;
use crate::scanner;
use crate::site::Site;

/// Per-mutant timeout used until a measured baseline × `--timeout-factor` lands
/// (T17). Generous on purpose: a false timeout is scored as Killed (A8) and would
/// silently inflate the score.
pub(crate) const DEFAULT_MUTANT_TIMEOUT: Duration = Duration::from_secs(300);

/// Runs the full single-file mutate loop and returns the bucket tallies.
///
/// # Errors
///
/// Returns an error if the target cannot be read or parsed, if the **baseline**
/// test suite is not green (see [`check_baseline`]), or if applying/restoring a
/// mutant fails.
pub(crate) fn run(
    target: &Path,
    crate_root: &Path,
    command: &[String],
    timeout: Duration,
) -> Result<MutationReport> {
    let guard = RestoreGuard::new(target)?;
    let sites = scanner::scan_source(guard.original())?;
    let runner = TestRunner::new(command, timeout, crate_root);

    check_baseline(&runner)?;
    mutate_sites(&guard, &sites, &runner)
}

/// Confirms the suite is green **before** any mutation.
///
/// The runner classifies a mutant purely by the test command's exit status, so an
/// already-red (or already-hanging) baseline would make every single mutant look
/// Killed. That is a misclassified run, not a useful one — abort instead.
///
/// # Errors
///
/// Returns an error if the baseline run fails to spawn, times out, or is red.
fn check_baseline(runner: &TestRunner) -> Result<()> {
    let baseline = runner
        .run()
        .context("failed to run the baseline test suite")?;
    if baseline.timed_out {
        bail!("baseline test suite timed out; every mutant would be misclassified as killed");
    }
    if !baseline.tests_passed {
        bail!(
            "baseline test suite is not green; fix the failing tests before mutating \
             (every mutant would otherwise be misclassified as killed)"
        );
    }
    Ok(())
}

/// Applies each site in turn, runs the suite, classifies the mutant, and restores
/// the original bytes — once per iteration, including on the error path.
///
/// # Errors
///
/// Returns an error if a site's span or mapping is invalid, if the mutant cannot
/// be written or restored, or if the test command fails to run.
fn mutate_sites(
    guard: &RestoreGuard,
    sites: &[Site],
    runner: &TestRunner,
) -> Result<MutationReport> {
    let mut report = MutationReport::default();

    for site in sites {
        // Everything up to `write_mutant` leaves the file pristine, so an early
        // `?` here needs no restore (and `Drop` is the backstop regardless).
        let token = guard
            .original()
            .get(site.byte_span.clone())
            .with_context(|| {
                format!(
                    "mutation site at line {} has a byte span outside the source",
                    site.line
                )
            })?;
        let replacement = operators::replacement(site.operator, token)?;
        let mutated = splice(guard.original(), &site.byte_span, &replacement)?;

        guard.write_mutant(&mutated)?;
        let outcome = runner.run_and_classify();
        // Restore before propagating a run error, so no iteration can exit with a
        // mutant left on disk.
        guard.restore()?;
        report.record(outcome?);
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::{check_baseline, mutate_sites, run};
    use crate::apply::RestoreGuard;
    use crate::runner::TestRunner;
    use crate::scanner;
    use std::path::Path;
    use std::time::Duration;

    /// A source with exactly two in-scope sites, one per function.
    const TWO_SITE_SOURCE: &str = "fn f(a: i32, b: i32) -> i32 { a + b }\n\
                                   fn g(c: i32, d: i32) -> i32 { c - d }\n";

    /// A program name no test machine can resolve, so spawning it always fails.
    const MISSING_PROGRAM: &str = "mutate4rust-no-such-test-command";

    /// Builds a portable shell command running `script`.
    fn shell(script: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".to_string(), "/C".to_string(), script.to_string()]
        } else {
            vec!["sh".to_string(), "-c".to_string(), script.to_string()]
        }
    }

    /// A command that appends the *current* content of `target.rs` to `log.txt`
    /// and then exits with `code` — so the log records exactly what was on disk
    /// for every invocation, one snapshot per invocation.
    fn log_target_then_exit(code: u8) -> Vec<String> {
        if cfg!(windows) {
            shell(&format!("type target.rs >> log.txt & exit {code}"))
        } else {
            shell(&format!("cat target.rs >> log.txt; exit {code}"))
        }
    }

    /// Like [`log_target_then_exit`], but blocks for ~5 seconds after logging so a
    /// short timeout always trips.
    fn log_target_then_block() -> Vec<String> {
        if cfg!(windows) {
            // ping -n 6 issues 6 pings ~1s apart ≈ 5s and needs no stdin.
            shell("type target.rs >> log.txt & ping -n 6 127.0.0.1 >NUL")
        } else {
            shell("cat target.rs >> log.txt; sleep 5")
        }
    }

    /// Asserts the command log holds exactly ONE invocation and that it saw the
    /// pristine source: proof that no mutant *test command* ever ran. Proving no
    /// mutant was ever *written* is the job of [`run_with_unwritable_target`].
    fn assert_baseline_ran_once_on_pristine_source(dir: &Path) {
        let log = std::fs::read_to_string(dir.join("log.txt")).expect("read command log");
        assert_eq!(
            log.replace("\r\n", "\n"),
            TWO_SITE_SOURCE,
            "expected exactly one invocation, of the unmutated source",
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("target.rs")).expect("read target"),
            TWO_SITE_SOURCE,
        );
    }

    /// Marks `path` read-only and reports whether writes are genuinely blocked.
    ///
    /// Windows honours the read-only attribute for every user; on Unix mode 0o444
    /// is ignored by an effectively-root process (common in containers), so we
    /// probe rather than assume. A `false` result means the "no write was
    /// attempted" proof is unavailable on this machine and callers skip it.
    fn make_read_only(path: &Path) -> bool {
        let mut perms = std::fs::metadata(path).expect("stat target").permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(path, perms).expect("mark target read-only");
        // Opening for write neither truncates nor creates — it just asks the OS
        // whether a write would be permitted.
        std::fs::OpenOptions::new().write(true).open(path).is_err()
    }

    /// Clears the read-only flag. Windows cannot delete a read-only file, so the
    /// tempdir teardown depends on this running before the dir is dropped.
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "restoring the pre-test permissions of a throwaway tempdir file"
    )]
    fn clear_read_only(path: &Path) {
        let mut perms = std::fs::metadata(path).expect("stat target").permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(path, perms).expect("clear read-only on target");
    }

    /// Drives [`run`] against a **read-only** `target.rs`, so any attempt to write
    /// a mutant fails loudly instead of silently succeeding-and-restoring, and
    /// returns the resulting error plus whether the OS actually blocked writes.
    fn run_with_unwritable_target(
        dir: &Path,
        command: &[String],
        timeout: Duration,
    ) -> (anyhow::Error, bool) {
        let target = dir.join("target.rs");
        let writes_blocked = make_read_only(&target);
        let err = run(&target, dir, command, timeout).expect_err("the baseline must abort the run");
        clear_read_only(&target);
        (err, writes_blocked)
    }

    /// Asserts `err` is the baseline abort and not a write/permission failure —
    /// i.e. the run bailed out *before* touching the target file.
    fn assert_is_baseline_error_not_a_write_error(
        err: &anyhow::Error,
        writes_blocked: bool,
        expected: &str,
    ) {
        assert!(
            err.to_string().contains(expected),
            "unhelpful message: {err}"
        );
        if writes_blocked {
            assert!(
                !format!("{err:#}").contains("write mutant"),
                "the run attempted to write a mutant before the baseline aborted: {err:#}",
            );
        }
    }

    /// Seeds `target.rs` in a temp dir and returns the dir plus a runner whose
    /// working directory is that dir.
    fn seed(source: &str, command: &[String]) -> (tempfile::TempDir, TestRunner) {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("target.rs"), source).expect("seed source");
        let runner = TestRunner::new(command, Duration::from_secs(60), dir.path());
        (dir, runner)
    }

    /// Runs the loop over `target.rs` in `dir` and returns the report.
    fn loop_over(dir: &Path, runner: &TestRunner) -> crate::report::MutationReport {
        let target = dir.join("target.rs");
        let guard = RestoreGuard::new(&target).expect("read target");
        let sites = scanner::scan_source(guard.original()).expect("parse target");
        mutate_sites(&guard, &sites, runner).expect("loop should succeed")
    }

    #[test]
    fn passing_suite_lets_every_mutant_survive() {
        let (dir, runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        let report = loop_over(dir.path(), &runner);

        assert_eq!(report.survived, 2);
        assert_eq!(report.killed, 0);
        assert_eq!(report.uncovered, 0, "coverage gating is S5, never S3");
    }

    #[test]
    fn failing_suite_kills_every_mutant() {
        let (dir, runner) = seed(TWO_SITE_SOURCE, &shell("exit 1"));
        let report = loop_over(dir.path(), &runner);

        assert_eq!(report.killed, 2);
        assert_eq!(report.survived, 0);
    }

    #[test]
    fn target_is_pristine_after_the_loop() {
        let (dir, runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        loop_over(dir.path(), &runner);

        assert_eq!(
            std::fs::read_to_string(dir.path().join("target.rs")).expect("read target"),
            TWO_SITE_SOURCE,
            "every iteration must restore the original bytes",
        );
    }

    #[test]
    fn mutations_never_accumulate_across_sites() {
        // Each mutant is spliced from the pristine in-memory original, so the file
        // on disk must carry exactly ONE mutation at a time.
        let (dir, runner) = seed(TWO_SITE_SOURCE, &log_target_then_exit(0));
        loop_over(dir.path(), &runner);

        let log = std::fs::read_to_string(dir.path().join("log.txt")).expect("read log");
        // Mutant 1 mutates only `+`, mutant 2 only `-`. Exactly one occurrence of
        // each across both snapshots proves no snapshot carried both mutations.
        assert_eq!(log.matches("a - b").count(), 1, "first mutant: {log}");
        assert_eq!(log.matches("c + d").count(), 1, "second mutant: {log}");
    }

    #[test]
    fn a_site_free_source_produces_an_empty_report() {
        let (dir, runner) = seed("fn f() {}\n", &shell("exit 0"));
        let report = loop_over(dir.path(), &runner);

        assert_eq!(report.score(), None);
        assert_eq!(report.killed + report.survived + report.uncovered, 0);
    }

    #[test]
    fn green_baseline_is_accepted() {
        let (_dir, runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        assert!(check_baseline(&runner).is_ok());
    }

    #[test]
    fn red_baseline_is_rejected() {
        let (_dir, runner) = seed(TWO_SITE_SOURCE, &shell("exit 1"));
        let err = check_baseline(&runner).expect_err("a red baseline must abort");
        assert!(
            err.to_string().contains("baseline"),
            "unhelpful message: {err}"
        );
    }

    #[test]
    fn red_baseline_aborts_before_any_mutation() {
        // Two independent proofs, because either alone has a hole:
        //  * the command log rules out any mutant *test command* running — exactly
        //    ONE invocation, and it saw unmutated source;
        //  * the read-only target rules out any mutant being *written* at all. A
        //    write-then-restore implementation would surface a permission error
        //    from `write_mutant` instead of the baseline error.
        let (dir, _runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        let (err, writes_blocked) = run_with_unwritable_target(
            dir.path(),
            &log_target_then_exit(1),
            Duration::from_secs(60),
        );

        assert_is_baseline_error_not_a_write_error(&err, writes_blocked, "baseline");
        assert_baseline_ran_once_on_pristine_source(dir.path());
    }

    #[test]
    fn timed_out_baseline_aborts_before_any_mutation() {
        // Same two proofs for the other abort branch: a baseline that never
        // finishes.
        let (dir, _runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        let (err, writes_blocked) = run_with_unwritable_target(
            dir.path(),
            &log_target_then_block(),
            Duration::from_millis(500),
        );

        assert_is_baseline_error_not_a_write_error(&err, writes_blocked, "timed out");
        assert_baseline_ran_once_on_pristine_source(dir.path());
    }

    #[test]
    fn a_runner_error_restores_the_target_before_propagating() {
        // The spawn failure happens *after* the first mutant is on disk. Reading
        // the file while `guard` is still alive proves the loop restored it
        // itself — `Drop` has not run yet and so cannot be what cleaned up.
        let (dir, runner) = seed(TWO_SITE_SOURCE, &[MISSING_PROGRAM.to_string()]);
        let target = dir.path().join("target.rs");
        let guard = RestoreGuard::new(&target).expect("read target");
        let sites = scanner::scan_source(guard.original()).expect("parse target");

        let err = mutate_sites(&guard, &sites, &runner).expect_err("spawn failure must propagate");
        assert!(
            err.to_string().contains("spawn"),
            "unhelpful message: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("read target"),
            TWO_SITE_SOURCE,
            "a mutant must never be left on disk when a runner error propagates",
        );
        drop(guard);
    }

    #[test]
    fn run_reports_survivors_end_to_end() {
        // Full composition (baseline + loop) against a stub test command: green
        // baseline, then every mutant survives.
        let (dir, _runner) = seed(TWO_SITE_SOURCE, &shell("exit 0"));
        let report = run(
            &dir.path().join("target.rs"),
            dir.path(),
            &shell("exit 0"),
            Duration::from_secs(60),
        )
        .expect("run should succeed");

        assert_eq!(report.survived, 2);
        assert_eq!(report.score(), Some(0.0));
    }

    #[test]
    fn an_unparseable_target_fails_fast() {
        let (dir, _runner) = seed("fn broken(", &shell("exit 0"));
        assert!(
            run(
                &dir.path().join("target.rs"),
                dir.path(),
                &shell("exit 0"),
                Duration::from_secs(60),
            )
            .is_err()
        );
    }
}
