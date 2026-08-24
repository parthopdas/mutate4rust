//! `cargo test` runner with a per-mutant timeout, plus mutant-outcome
//! classification.
//!
//! This module is split into a **pure** classifier and a thin **infrastructure**
//! runner (design.md → the Runner; the only layer touching process/fs):
//! - [`classify`] is a pure decision over two booleans — no subprocess — so the
//!   killed/survived logic is unit-testable in isolation.
//! - [`TestRunner`] spawns the test command in a working directory, waits with a
//!   timeout, and folds the result into a [`MutantOutcome`].
//!
//! # Classification semantics (mutate4go parity — decision A8)
//!
//! Per mutant, after the mutated source is on disk (via [`crate::apply::RestoreGuard`]):
//! - tests **pass** (exit 0) → [`MutantOutcome::Survived`] — the mutant was not detected.
//! - tests **fail** (non-zero exit) → [`MutantOutcome::Killed`] — the mutant was detected.
//! - a **non-compiling** mutant makes `cargo test` exit non-zero → [`MutantOutcome::Killed`]
//!   (folded into killed, per A8 — not a separate bucket).
//! - a **timeout** (mutant likely introduced a hang) → [`MutantOutcome::Killed`] (A8).
//!
//! [`MutantOutcome::Uncovered`] is reserved here but **never produced** by T8 —
//! coverage gating lands in a later slice (S5).
//!
//! # Durability assumption
//!
//! The target `.rs` file is assumed to be **under version control**. The
//! [`crate::apply::RestoreGuard`] keeps the original bytes in memory only, so a
//! hard kill (SIGKILL/OOM/power-loss) mid-run — when `Drop` never runs — can leave
//! a mutant as the final on-disk state. Recovery is `git checkout -- <file>`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use wait_timeout::ChildExt;

/// The outcome of testing a single mutant.
///
/// [`Uncovered`](MutantOutcome::Uncovered) is reserved for coverage gating (S5)
/// and is never produced by the T8 runner, which only ever yields
/// [`Killed`](MutantOutcome::Killed) or [`Survived`](MutantOutcome::Survived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutantOutcome {
    /// The mutant was detected — a test failed, the mutant did not compile, or the
    /// test run timed out (all folded into killed, per A8).
    Killed,
    /// The mutant was not detected — the tests still passed with it applied.
    Survived,
    /// The mutant's site is not exercised by the test suite. Reserved for coverage
    /// gating (S5); never produced by the T8 runner.
    Uncovered,
}

/// The raw result of one test-command execution, before classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawRunOutcome {
    /// The command exited with a success status (exit 0).
    pub tests_passed: bool,
    /// The command was killed because it exceeded the timeout.
    pub timed_out: bool,
}

/// Classifies a mutant from the raw test result.
///
/// A timeout is always [`Killed`](MutantOutcome::Killed) (the mutant likely hung),
/// regardless of any exit status; otherwise passing tests mean the mutant
/// [`Survived`](MutantOutcome::Survived) and failing tests mean it was
/// [`Killed`](MutantOutcome::Killed).
#[must_use]
pub fn classify(tests_passed: bool, timed_out: bool) -> MutantOutcome {
    if timed_out {
        MutantOutcome::Killed
    } else if tests_passed {
        MutantOutcome::Survived
    } else {
        MutantOutcome::Killed
    }
}

/// The default test command: `cargo test`.
///
/// Overridable via [`TestRunner::new`] — parity `--test-command` wiring lands in a
/// later slice (T17).
#[must_use]
pub fn default_command() -> Vec<String> {
    vec!["cargo".to_string(), "test".to_string()]
}

/// Runs a test command against the crate under test with a per-mutant timeout.
///
/// Takes domain primitives (a command vector, a [`Duration`], a working
/// directory) — never the clap `Cli`/`RunConfig` (YAGNI until flag-wiring in a
/// later slice).
pub struct TestRunner {
    command: Vec<String>,
    timeout: Duration,
    working_dir: PathBuf,
}

impl TestRunner {
    /// Creates a runner for `command` (first element is the program, the rest are
    /// arguments), run in `working_dir` with a per-run `timeout`.
    #[must_use]
    pub fn new(command: &[String], timeout: Duration, working_dir: &Path) -> Self {
        Self {
            command: command.to_vec(),
            timeout,
            working_dir: working_dir.to_path_buf(),
        }
    }

    /// Runs the test command, waiting at most `timeout`.
    ///
    /// On timeout the child is killed and `timed_out` is set. Note: killing the
    /// child may not reap grandchildren (`cargo` spawns the test binary) — that is
    /// acceptable for T8.
    ///
    /// # Errors
    ///
    /// Returns an error if the command is empty, if the process cannot be spawned,
    /// or if waiting on it fails.
    pub fn run(&self) -> Result<RawRunOutcome> {
        let (program, args) = self
            .command
            .split_first()
            .context("test command is empty")?;
        ensure!(!program.is_empty(), "test command program is empty");

        let mut child = Command::new(program)
            .args(args)
            .current_dir(&self.working_dir)
            .spawn()
            .with_context(|| format!("failed to spawn test command `{program}`"))?;

        match child
            .wait_timeout(self.timeout)
            .with_context(|| format!("failed to wait on test command `{program}`"))?
        {
            Some(status) => Ok(RawRunOutcome {
                tests_passed: status.success(),
                timed_out: false,
            }),
            None => {
                // Timed out: kill the child and reap it so it does not linger.
                // TODO(T16/T17): robust process-tree/job-object kill on timeout —
                // child.kill() may not reap grandchildren (cargo spawns the test binary).
                child
                    .kill()
                    .with_context(|| format!("failed to kill timed-out command `{program}`"))?;
                child
                    .wait()
                    .with_context(|| format!("failed to reap timed-out command `{program}`"))?;
                Ok(RawRunOutcome {
                    tests_passed: false,
                    timed_out: true,
                })
            }
        }
    }

    /// Runs the test command and classifies the result into a [`MutantOutcome`].
    ///
    /// # Errors
    ///
    /// Returns an error if [`run`](TestRunner::run) fails.
    pub fn run_and_classify(&self) -> Result<MutantOutcome> {
        let outcome = self.run()?;
        Ok(classify(outcome.tests_passed, outcome.timed_out))
    }
}

#[cfg(test)]
mod tests {
    use super::{MutantOutcome, TestRunner, classify, default_command};
    use std::time::Duration;

    #[test]
    fn classify_passing_tests_survives() {
        assert_eq!(classify(true, false), MutantOutcome::Survived);
    }

    #[test]
    fn classify_failing_tests_kills() {
        assert_eq!(classify(false, false), MutantOutcome::Killed);
    }

    #[test]
    fn classify_timeout_kills_regardless_of_exit() {
        // A timeout is Killed whether or not the process had exited successfully.
        assert_eq!(classify(true, true), MutantOutcome::Killed);
        assert_eq!(classify(false, true), MutantOutcome::Killed);
    }

    #[test]
    fn default_command_is_cargo_test() {
        assert_eq!(default_command(), vec!["cargo", "test"]);
    }

    /// Builds a portable shell command running `script`.
    fn shell(script: &str) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".to_string(), "/C".to_string(), script.to_string()]
        } else {
            vec!["sh".to_string(), "-c".to_string(), script.to_string()]
        }
    }

    /// A portable command that blocks for ~5 seconds.
    fn slow_command() -> Vec<String> {
        if cfg!(windows) {
            // ping -n 6 issues 6 pings ~1s apart ≈ 5s, and needs no interactive input.
            shell("ping -n 6 127.0.0.1 >NUL")
        } else {
            shell("sleep 5")
        }
    }

    #[test]
    fn run_exit_zero_survives() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let runner = TestRunner::new(&shell("exit 0"), Duration::from_secs(30), dir.path());

        let raw = runner.run().expect("run command");
        assert!(raw.tests_passed);
        assert!(!raw.timed_out);
        assert_eq!(
            runner.run_and_classify().expect("classify"),
            MutantOutcome::Survived
        );
    }

    #[test]
    fn run_exit_nonzero_kills() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let runner = TestRunner::new(&shell("exit 1"), Duration::from_secs(30), dir.path());

        let raw = runner.run().expect("run command");
        assert!(!raw.tests_passed);
        assert!(!raw.timed_out);
        assert_eq!(
            runner.run_and_classify().expect("classify"),
            MutantOutcome::Killed
        );
    }

    #[test]
    fn run_timeout_kills() {
        let dir = tempfile::tempdir().expect("create temp dir");
        // Short timeout vs a ~5s command: the timeout trips well before it ends, so
        // the test is fast and non-flaky (we never wait for the sleep to finish).
        let runner = TestRunner::new(&slow_command(), Duration::from_millis(300), dir.path());

        let raw = runner.run().expect("run command");
        assert!(raw.timed_out);
        assert!(!raw.tests_passed);
        assert_eq!(
            runner.run_and_classify().expect("classify"),
            MutantOutcome::Killed
        );
    }
}
