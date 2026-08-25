//! `cargo test` runner with a per-mutant timeout, plus mutant-outcome
//! classification.
//!
//! This module is split into a **pure** classifier and a thin **infrastructure**
//! runner (design.md → the Runner; the only layer touching process/fs):
//! - [`classify`] is a pure decision over a [`RawRunOutcome`] — no subprocess — so
//!   the killed/survived logic is unit-testable in isolation.
//! - [`TestRunner`] spawns the test command in a working directory, waits with a
//!   timeout, and folds the result into a [`MutantResult`] (the buckets live in
//!   the pure [`crate::outcome`] module, so runner and reporter both depend
//!   inward on the same type).
//!
//! # Classification semantics (mutate4go parity — decision A8)
//!
//! Per mutant, after the mutated source is on disk (via [`crate::apply::RestoreGuard`]):
//! - tests **pass** (exit 0) → [`MutantResult::Survived`] — the mutant was not detected.
//! - tests **fail** (non-zero exit) → [`MutantResult::Killed`] — the mutant was detected.
//! - a **non-compiling** mutant makes `cargo test` exit non-zero → `Killed`
//!   (folded into killed, per A8 — not a separate bucket).
//! - a **timeout** (mutant likely introduced a hang) → `Killed` (A8).
//!
//! Every kill carries a [`KillReason`], which enriches the per-mutant *record*
//! and never changes the bucket. [`crate::outcome::MutantOutcome::Uncovered`] is
//! never produced here — coverage gating is the pipeline's job (S5), decided
//! before a mutant is ever written.
//!
//! # Why stderr is captured
//!
//! Telling an **arithmetic-panic** kill (risk R3: debug-build overflow,
//! divide-by-zero) apart from an ordinary test failure needs the child's panic
//! message, so stderr is piped and drained by a reader thread — draining
//! *concurrently* is what stops a full pipe buffer from deadlocking
//! `wait_timeout`. stdout stays inherited, so the user still sees test output.
//! The captured stderr is also what makes a red-baseline abort diagnosable.
//!
//! # Durability assumption
//!
//! The target `.rs` file is assumed to be **under version control**. The
//! [`crate::apply::RestoreGuard`] keeps the original bytes in memory only, so a
//! hard kill (SIGKILL/OOM/power-loss) mid-run — when `Drop` never runs — can leave
//! a mutant as the final on-disk state. Recovery is `git checkout -- <file>`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use wait_timeout::ChildExt;

use crate::outcome::{KillReason, MutantResult};

/// The panic messages Rust's arithmetic operations emit, which risk R3 predicts
/// mutated arithmetic will trip. Matched as substrings of the child's stderr.
///
/// `"with overflow"` is the shared tail of `attempt to add/subtract/multiply …
/// with overflow`, so one marker covers every overflowing operation.
const ARITHMETIC_PANICS: [&str; 3] = [
    "with overflow",
    "attempt to divide by zero",
    "attempt to calculate the remainder with a divisor of zero",
];

/// The raw result of one test-command execution, before classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRunOutcome {
    /// The command exited with a success status (exit 0).
    pub tests_passed: bool,
    /// The command was killed because it exceeded the timeout.
    pub timed_out: bool,
    /// Everything the command wrote to stderr. Empty on the timeout path, where
    /// the pipe is abandoned rather than waited on (see [`TestRunner::run`]).
    pub stderr: String,
}

/// Whether `stderr` shows one of Rust's arithmetic runtime panics (R3).
fn is_arithmetic_panic(stderr: &str) -> bool {
    ARITHMETIC_PANICS
        .iter()
        .any(|marker| stderr.contains(marker))
}

/// Classifies a mutant from the raw test result.
///
/// A timeout is always killed (the mutant likely hung), regardless of any exit
/// status; otherwise passing tests mean the mutant survived, and failing tests
/// mean it was killed — with [`KillReason::ArithmeticPanic`] when the captured
/// stderr shows Rust tripped an arithmetic panic rather than an assertion.
#[must_use]
pub fn classify(raw: &RawRunOutcome) -> MutantResult {
    if raw.timed_out {
        MutantResult::Killed(KillReason::Timeout)
    } else if raw.tests_passed {
        MutantResult::Survived
    } else if is_arithmetic_panic(&raw.stderr) {
        MutantResult::Killed(KillReason::ArithmeticPanic)
    } else {
        MutantResult::Killed(KillReason::TestFailure)
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

    /// Runs the test command, waiting at most `timeout`, and captures its stderr.
    ///
    /// On timeout the child is killed and `timed_out` is set; the stderr reader is
    /// **detached** rather than joined, because a hung grandchild can still hold
    /// the pipe's write end open and joining would reintroduce the hang the
    /// timeout exists to escape. Note also that killing the child may not reap
    /// grandchildren (`cargo` spawns the test binary) — process-tree kill is owed
    /// to T17.
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
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn test command `{program}`"))?;

        // Drain stderr concurrently: a pipe left unread blocks the child once its
        // buffer fills, which `wait_timeout` would then report as a timeout.
        let reader = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer);
                buffer
            })
        });

        match child
            .wait_timeout(self.timeout)
            .with_context(|| format!("failed to wait on test command `{program}`"))?
        {
            Some(status) => {
                let stderr = reader
                    .and_then(|handle| handle.join().ok())
                    .unwrap_or_default();
                Ok(RawRunOutcome {
                    tests_passed: status.success(),
                    timed_out: false,
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                })
            }
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
                drop(reader);
                Ok(RawRunOutcome {
                    tests_passed: false,
                    timed_out: true,
                    stderr: String::new(),
                })
            }
        }
    }

    /// Runs the test command and classifies the result into a [`MutantResult`].
    ///
    /// # Errors
    ///
    /// Returns an error if [`run`](TestRunner::run) fails.
    pub fn run_and_classify(&self) -> Result<MutantResult> {
        Ok(classify(&self.run()?))
    }
}

#[cfg(test)]
mod tests {
    use super::{RawRunOutcome, TestRunner, classify, default_command, is_arithmetic_panic};
    use crate::outcome::{KillReason, MutantResult};
    use std::time::Duration;

    /// Builds a raw outcome with the given stderr and a failing exit.
    fn failed_with(stderr: &str) -> RawRunOutcome {
        RawRunOutcome {
            tests_passed: false,
            timed_out: false,
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn classify_passing_tests_survives() {
        assert_eq!(
            classify(&RawRunOutcome {
                tests_passed: true,
                timed_out: false,
                stderr: String::new(),
            }),
            MutantResult::Survived
        );
    }

    #[test]
    fn classify_failing_tests_kills_with_a_test_failure_reason() {
        assert_eq!(
            classify(&failed_with("assertion `left == right` failed")),
            MutantResult::Killed(KillReason::TestFailure)
        );
    }

    #[test]
    fn classify_timeout_kills_regardless_of_exit() {
        // A timeout is Killed whether or not the process had exited successfully,
        // and outranks any stderr content.
        for tests_passed in [true, false] {
            assert_eq!(
                classify(&RawRunOutcome {
                    tests_passed,
                    timed_out: true,
                    stderr: "attempt to divide by zero".to_owned(),
                }),
                MutantResult::Killed(KillReason::Timeout)
            );
        }
    }

    /// The R3 signal: each of Rust's arithmetic panic messages is recognised, and
    /// the bucket is still Killed (A8 unchanged).
    #[test]
    fn classify_recognises_each_arithmetic_panic() {
        for stderr in [
            "thread 'main' panicked at src/lib.rs:4:5:\nattempt to add with overflow",
            "thread 'main' panicked at src/lib.rs:4:5:\nattempt to multiply with overflow",
            "thread 'main' panicked at src/lib.rs:4:5:\nattempt to divide by zero",
            "thread 'main' panicked at src/lib.rs:4:5:\n\
             attempt to calculate the remainder with a divisor of zero",
        ] {
            let result = classify(&failed_with(stderr));
            assert_eq!(
                result,
                MutantResult::Killed(KillReason::ArithmeticPanic),
                "unrecognised arithmetic panic: {stderr}"
            );
            assert_eq!(result.bucket(), crate::outcome::MutantOutcome::Killed);
        }
    }

    /// An assertion panic is a panic too — it must NOT be mistaken for the
    /// arithmetic kind, or the R3/D6 signal would be meaningless.
    #[test]
    fn an_assertion_panic_is_not_an_arithmetic_panic() {
        assert!(!is_arithmetic_panic(
            "thread 'main' panicked at src/lib.rs:9:5:\nassertion failed: a == b"
        ));
        assert!(!is_arithmetic_panic("error: could not compile `covfix`"));
        assert!(!is_arithmetic_panic(""));
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
            MutantResult::Survived
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
            MutantResult::Killed(KillReason::TestFailure)
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
            MutantResult::Killed(KillReason::Timeout)
        );
    }

    /// The capture is real, end-to-end through a live process: a child that
    /// prints an arithmetic panic to **stderr** and exits non-zero is classified
    /// as an arithmetic-panic kill, while the same message on **stdout** is not
    /// (proving stderr specifically is what is read).
    #[test]
    fn a_real_child_process_arithmetic_panic_is_captured_from_stderr() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let to_stderr = if cfg!(windows) {
            shell("echo attempt to divide by zero 1>&2 & exit 101")
        } else {
            shell("echo 'attempt to divide by zero' 1>&2; exit 101")
        };
        let runner = TestRunner::new(&to_stderr, Duration::from_secs(30), dir.path());
        let raw = runner.run().expect("run command");
        assert!(
            raw.stderr.contains("attempt to divide by zero"),
            "stderr was not captured: {:?}",
            raw.stderr
        );
        assert_eq!(
            classify(&raw),
            MutantResult::Killed(KillReason::ArithmeticPanic)
        );

        let to_stdout = if cfg!(windows) {
            shell("echo attempt to divide by zero & exit 101")
        } else {
            shell("echo 'attempt to divide by zero'; exit 101")
        };
        let runner = TestRunner::new(&to_stdout, Duration::from_secs(30), dir.path());
        assert_eq!(
            runner.run_and_classify().expect("classify"),
            MutantResult::Killed(KillReason::TestFailure),
            "stdout must not be mistaken for stderr",
        );
    }

    /// A child that writes far more stderr than any pipe buffer holds must still
    /// complete — the concurrent drain is what prevents a deadlock being reported
    /// as a timeout.
    #[test]
    fn a_flood_of_stderr_does_not_deadlock_the_wait() {
        let dir = tempfile::tempdir().expect("create temp dir");
        // ~512 KiB, an order of magnitude past a typical 64 KiB pipe buffer. The
        // bytes come from a file so the child is ONE fast process — looping a
        // shell `echo` produces the same flood but takes ~a minute on Windows,
        // which would make a deadlock and mere slowness indistinguishable.
        std::fs::write(dir.path().join("flood.txt"), "x".repeat(512 * 1024))
            .expect("seed flood file");
        let flood = if cfg!(windows) {
            shell("type flood.txt 1>&2")
        } else {
            shell("cat flood.txt >&2")
        };
        let runner = TestRunner::new(&flood, Duration::from_secs(60), dir.path());

        let raw = runner.run().expect("run command");
        assert!(!raw.timed_out, "the drain must not deadlock into a timeout");
        assert!(
            raw.stderr.len() > 256 * 1024,
            "captured {}",
            raw.stderr.len()
        );
    }
}
