//! Killed / Survived / Uncovered result reporter (Application layer).
//!
//! Pure tallying over [`MutantOutcome`] — no fs, process, or clap; the caller
//! decides where the rendered text goes. The three buckets are **strict
//! mutate4go parity** (assumption A8: timed-out and non-compiling mutants fold
//! into Killed, with no `invalid` bucket). The numeric **score** is an additive
//! mutate4rust extension — upstream emits raw counts only.
//!
//! Score = `killed / (killed + survived)`. Uncovered mutants are excluded from
//! the denominator and reported separately, so gaining coverage never silently
//! deflates the score.

use crate::outcome::MutantOutcome;

/// Bucket tallies for one mutation run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MutationReport {
    /// Mutants detected by the suite (incl. timeouts and non-compiling mutants).
    pub(crate) killed: usize,
    /// Mutants the suite failed to detect.
    pub(crate) survived: usize,
    /// Sites not exercised by the suite — reported, never executed (S5).
    pub(crate) uncovered: usize,
}

impl MutationReport {
    /// Adds one mutant outcome to its bucket.
    pub(crate) fn record(&mut self, outcome: MutantOutcome) {
        match outcome {
            MutantOutcome::Killed => self.killed += 1,
            MutantOutcome::Survived => self.survived += 1,
            MutantOutcome::Uncovered => self.uncovered += 1,
        }
    }

    /// The mutation score as a ratio in `0.0..=1.0`, or `None` when no mutant was
    /// actually run (`killed + survived == 0`) — there is no meaningful score to
    /// report, and no division by zero.
    pub(crate) fn score(&self) -> Option<f64> {
        let scored = self.killed + self.survived;
        (scored > 0).then(|| self.killed as f64 / scored as f64)
    }

    /// A stable, greppable multi-line summary (trailing newline included).
    pub(crate) fn summary(&self) -> String {
        let score = self.score().map_or_else(
            || "n/a (no mutants were run)".to_owned(),
            |score| format!("{:.1}%", score * 100.0),
        );
        format!(
            "Killed:    {}\n\
             Survived:  {}\n\
             Uncovered: {}\n\
             Score:     {score}\n",
            self.killed, self.survived, self.uncovered,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::MutationReport;
    use crate::outcome::MutantOutcome;

    /// Builds a report by recording the given outcomes in order.
    fn report_of(outcomes: &[MutantOutcome]) -> MutationReport {
        let mut report = MutationReport::default();
        for outcome in outcomes {
            report.record(*outcome);
        }
        report
    }

    #[test]
    fn each_outcome_lands_in_its_own_bucket() {
        let report = report_of(&[
            MutantOutcome::Killed,
            MutantOutcome::Killed,
            MutantOutcome::Survived,
            MutantOutcome::Uncovered,
            MutantOutcome::Uncovered,
            MutantOutcome::Uncovered,
        ]);

        assert_eq!(report.killed, 2);
        assert_eq!(report.survived, 1);
        assert_eq!(report.uncovered, 3);
    }

    #[test]
    fn score_is_killed_over_killed_plus_survived() {
        let report = report_of(&[
            MutantOutcome::Killed,
            MutantOutcome::Killed,
            MutantOutcome::Killed,
            MutantOutcome::Survived,
        ]);

        assert_eq!(report.score(), Some(0.75));
    }

    #[test]
    fn uncovered_mutants_are_excluded_from_the_score() {
        // Same killed/survived tallies, wildly different uncovered counts — the
        // score must not move (uncovered is reported separately, not scored).
        let without = report_of(&[MutantOutcome::Killed, MutantOutcome::Survived]);
        let with = report_of(&[
            MutantOutcome::Killed,
            MutantOutcome::Survived,
            MutantOutcome::Uncovered,
            MutantOutcome::Uncovered,
        ]);

        assert_eq!(without.score(), Some(0.5));
        assert_eq!(with.score(), without.score());
    }

    #[test]
    fn score_is_none_when_no_mutant_was_run() {
        // The divide-by-zero edge: an empty run, and a run that was entirely
        // uncovered, both have no scoreable mutant.
        assert_eq!(MutationReport::default().score(), None);
        assert_eq!(report_of(&[MutantOutcome::Uncovered]).score(), None);
    }

    #[test]
    fn score_extremes_are_zero_and_one() {
        assert_eq!(report_of(&[MutantOutcome::Killed]).score(), Some(1.0));
        assert_eq!(report_of(&[MutantOutcome::Survived]).score(), Some(0.0));
    }

    #[test]
    fn summary_is_stable_and_greppable() {
        let report = report_of(&[
            MutantOutcome::Killed,
            MutantOutcome::Killed,
            MutantOutcome::Killed,
            MutantOutcome::Survived,
            MutantOutcome::Uncovered,
        ]);

        assert_eq!(
            report.summary(),
            "Killed:    3\n\
             Survived:  1\n\
             Uncovered: 1\n\
             Score:     75.0%\n",
        );
    }

    #[test]
    fn summary_reports_no_score_when_nothing_ran() {
        assert_eq!(
            MutationReport::default().summary(),
            "Killed:    0\n\
             Survived:  0\n\
             Uncovered: 0\n\
             Score:     n/a (no mutants were run)\n",
        );
    }
}
