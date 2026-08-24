//! The outcome of testing a single mutant (Core/domain).
//!
//! This module is **pure** (design.md → inward dependency flow): no fs, process,
//! argv, or clap. It gives the killed/survived/uncovered buckets a Core home so
//! that both the infrastructure runner ([`crate::runner`]) and the application
//! reporter ([`crate::report`]) depend *inward* on the same type.

/// The outcome of testing a single mutant.
///
/// The three variants are the report buckets, in strict mutate4go parity
/// (assumption A8): a timed-out or non-compiling mutant is folded into
/// [`Killed`](MutantOutcome::Killed) — there is deliberately **no** `invalid`
/// bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutantOutcome {
    /// The mutant was detected — a test failed, the mutant did not compile, or the
    /// test run timed out (all folded into killed, per A8).
    Killed,
    /// The mutant was not detected — the tests still passed with it applied.
    Survived,
    /// The mutant's site is not exercised by the test suite. Reserved for coverage
    /// gating (S5); never produced by the S3 runner or mutate loop.
    Uncovered,
}
