//! The outcome of testing a single mutant (Core/domain).
//!
//! This module is **pure** (design.md → inward dependency flow): no fs, process,
//! argv, or clap. It gives the killed/survived/uncovered buckets a Core home so
//! that both the infrastructure runner ([`crate::runner`]) and the application
//! reporter ([`crate::report`]) depend *inward* on the same type.
//!
//! # Two levels, deliberately
//!
//! [`MutantOutcome`] is the **report bucket** — three variants, strict mutate4go
//! parity (A8). [`MutantResult`] is what a per-mutant *record* stores: the same
//! three cases, but with the kill carrying a [`KillReason`]. The bucket is
//! **derived** from the result ([`MutantResult::bucket`]), so richer records can
//! never split, rename, or add a bucket.

/// Why a mutant was killed.
///
/// A8 is unchanged — every variant folds into [`MutantOutcome::Killed`]. The
/// distinction exists to inform deferral **D6** (release-mode / overflow toggle):
/// risk R3 predicts that Rust's debug-build arithmetic panics kill mutants
/// trivially, with no signal about test quality, and a bucket count cannot tell
/// us how often that happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillReason {
    /// The test command reported failure — an assertion, a compile error, or any
    /// other non-zero exit that was not an arithmetic panic.
    TestFailure,
    /// The mutant tripped one of Rust's **arithmetic** runtime panics (debug
    /// overflow, divide-by-zero, remainder-by-zero) — risk R3's trivial kill.
    ArithmeticPanic,
    /// The test command exceeded the per-mutant timeout and was killed.
    Timeout,
}

impl KillReason {
    /// A short, stable label for rendering in a report.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            KillReason::TestFailure => "test-failure",
            KillReason::ArithmeticPanic => "arithmetic-panic",
            KillReason::Timeout => "timeout",
        }
    }
}

/// What happened at one mutation site, at full record fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutantResult {
    /// The mutant was detected; the reason is recorded but does **not** change
    /// the bucket (A8).
    Killed(KillReason),
    /// The mutant was not detected — the tests still passed with it applied.
    Survived,
    /// The site is not exercised by the test suite, so no mutant was ever built:
    /// reported, never executed (A6/S5).
    Uncovered,
}

impl MutantResult {
    /// The report bucket this result falls into.
    #[must_use]
    pub fn bucket(self) -> MutantOutcome {
        match self {
            MutantResult::Killed(_) => MutantOutcome::Killed,
            MutantResult::Survived => MutantOutcome::Survived,
            MutantResult::Uncovered => MutantOutcome::Uncovered,
        }
    }
}

/// The report bucket a mutant lands in.
///
/// The three variants are strict mutate4go parity (assumption A8): a timed-out or
/// non-compiling mutant is folded into [`Killed`](MutantOutcome::Killed) — there
/// is deliberately **no** `invalid` bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutantOutcome {
    /// The mutant was detected — a test failed, the mutant did not compile, or the
    /// test run timed out (all folded into killed, per A8).
    Killed,
    /// The mutant was not detected — the tests still passed with it applied.
    Survived,
    /// The mutant's site is not exercised by the test suite (coverage gating, S5).
    Uncovered,
}

#[cfg(test)]
mod tests {
    use super::{KillReason, MutantOutcome, MutantResult};

    /// Every kill reason, for exhaustive iteration in tests.
    const REASONS: [KillReason; 3] = [
        KillReason::TestFailure,
        KillReason::ArithmeticPanic,
        KillReason::Timeout,
    ];

    /// Every kill reason folds into the SAME bucket — A8 is not weakened by the
    /// added record fidelity.
    #[test]
    fn every_kill_reason_folds_into_the_killed_bucket() {
        for reason in REASONS {
            assert_eq!(
                MutantResult::Killed(reason).bucket(),
                MutantOutcome::Killed,
                "{reason:?} must not create a fourth bucket",
            );
        }
    }

    #[test]
    fn the_other_results_map_onto_their_own_buckets() {
        assert_eq!(MutantResult::Survived.bucket(), MutantOutcome::Survived);
        assert_eq!(MutantResult::Uncovered.bucket(), MutantOutcome::Uncovered);
    }

    /// Distinguishability is the whole point: two reasons that rendered the same
    /// label would be indistinguishable in a report.
    #[test]
    fn kill_reason_labels_are_distinct() {
        let mut labels: Vec<&str> = REASONS.iter().map(|reason| reason.label()).collect();
        labels.sort_unstable();
        let before = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), before, "duplicate kill-reason label");
    }
}
