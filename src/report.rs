//! Per-mutant records and the Killed / Survived / Uncovered reporter
//! (Application layer).
//!
//! Pure accumulation over [`MutantRecord`]s — no fs, process, or clap; the caller
//! decides where the rendered text goes. The three buckets are **strict
//! mutate4go parity** (assumption A8: timed-out and non-compiling mutants fold
//! into Killed, with no `invalid` bucket). The numeric **score** is an additive
//! mutate4rust extension — upstream emits raw counts only.
//!
//! Score = `killed / (killed + survived)`. Uncovered sites are excluded from the
//! denominator and reported separately, so gaining coverage never silently
//! deflates the score — and the rendered `Score:` line carries that denominator
//! and the exclusion inline, so a partially-covered file cannot be misread as a
//! fully-tested one.
//!
//! # Records are the model; counters are derived
//!
//! A survivor count with no survivor **location** is not actionable, and A6
//! requires uncovered sites to be *listed*, which no counter can express. So the
//! report stores one [`MutantRecord`] per site — line, operator, and result —
//! and [`killed`](MutationReport::killed) / [`survived`](MutationReport::survived)
//! / [`uncovered`](MutationReport::uncovered) are computed from them.
//! [`summary`](MutationReport::summary) is a **rendering** of that model, and a
//! change-detector rather than a stability contract pre-1.0; a future structured
//! report (D4) must serialize the records, never re-parse the summary.
//!
//! The run is single-file (A3), so the file is held once on the report rather
//! than copied into every record; a record renders as `file:line`.

use crate::outcome::{KillReason, MutantOutcome, MutantResult};
use crate::site::{Operator, Site};

/// What happened at one mutation site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MutantRecord {
    /// 1-based line of the site within the reported file.
    pub(crate) line: usize,
    /// The operator that was (or would have been) mutated.
    pub(crate) operator: Operator,
    /// The outcome, at record fidelity — a kill carries its [`KillReason`].
    pub(crate) result: MutantResult,
}

impl MutantRecord {
    /// Records `result` for `site`.
    pub(crate) fn new(site: &Site, result: MutantResult) -> Self {
        Self {
            line: site.line,
            operator: site.operator,
            result,
        }
    }

    /// The record's bucket.
    pub(crate) fn bucket(&self) -> MutantOutcome {
        self.result.bucket()
    }

    /// One rendered line, e.g. ``src/lib.rs:11 Arithmetic `*` (arithmetic-panic)``.
    fn render(&self, file: &str) -> String {
        let reason = match self.result {
            MutantResult::Killed(reason) => format!(" ({})", reason.label()),
            MutantResult::Survived | MutantResult::Uncovered => String::new(),
        };
        format!(
            "  {file}:{} {:?} `{}`{reason}",
            self.line,
            self.operator.kind(),
            self.operator.canonical_token(),
        )
    }
}

/// Every per-mutant record for one single-file mutation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MutationReport {
    file: String,
    records: Vec<MutantRecord>,
}

impl MutationReport {
    /// An empty report for `file` (as it should be displayed to the user).
    pub(crate) fn new(file: &str) -> Self {
        Self {
            file: file.to_owned(),
            records: Vec::new(),
        }
    }

    /// Adds one record.
    pub(crate) fn record(&mut self, record: MutantRecord) {
        self.records.push(record);
    }

    /// Every record, in the order they were added.
    #[cfg(test)]
    pub(crate) fn records(&self) -> &[MutantRecord] {
        &self.records
    }

    /// Mutants detected by the suite (incl. timeouts and non-compiling mutants).
    pub(crate) fn killed(&self) -> usize {
        self.count(MutantOutcome::Killed)
    }

    /// Mutants the suite failed to detect.
    pub(crate) fn survived(&self) -> usize {
        self.count(MutantOutcome::Survived)
    }

    /// Sites not exercised by the suite — reported, never executed (A6/S5).
    pub(crate) fn uncovered(&self) -> usize {
        self.count(MutantOutcome::Uncovered)
    }

    /// How many records fall in `bucket`.
    fn count(&self, bucket: MutantOutcome) -> usize {
        self.records
            .iter()
            .filter(|record| record.bucket() == bucket)
            .count()
    }

    /// How many kills were attributed to `reason`.
    pub(crate) fn killed_by(&self, reason: KillReason) -> usize {
        self.records
            .iter()
            .filter(|record| record.result == MutantResult::Killed(reason))
            .count()
    }

    /// The mutation score as a ratio in `0.0..=1.0`, or `None` when no mutant was
    /// actually run (`killed + survived == 0`) — there is no meaningful score to
    /// report, and no division by zero.
    pub(crate) fn score(&self) -> Option<f64> {
        let killed = self.killed();
        let scored = killed + self.survived();
        (scored > 0).then(|| killed as f64 / scored as f64)
    }

    /// The rendered `Score:` value: the percentage, the fraction of mutants it
    /// was computed from, and — when there were any — the uncovered sites
    /// excluded from that denominator.
    ///
    /// A bare `100.0%` on a partially-covered file reads as "this file is well
    /// tested"; carrying the denominator and the exclusion inline makes that
    /// misread impossible. Zero uncovered sites say nothing about exclusion, and
    /// a zero denominator says there was no score to compute rather than
    /// dividing by zero.
    fn score_line(&self) -> String {
        let scored = self.killed() + self.survived();
        let score = match self.score() {
            Some(score) => format!(
                "{:.1}% ({} killed of {scored} {} run",
                score * 100.0,
                self.killed(),
                if scored == 1 { "mutant" } else { "mutants" },
            ),
            None => "n/a (no mutants were run".to_owned(),
        };
        let excluded = match self.uncovered() {
            0 => String::new(),
            1 => "; 1 uncovered site excluded".to_owned(),
            uncovered => format!("; {uncovered} uncovered sites excluded"),
        };
        format!("{score}{excluded})")
    }

    /// A greppable multi-line rendering of the records (trailing newline
    /// included): the bucket tallies with the kill breakdown, then the survivor
    /// and uncovered listings — each omitted when empty.
    pub(crate) fn summary(&self) -> String {
        let mut summary = format!(
            "Killed:    {} ({} {}, {} {}, {} {})\n\
             Survived:  {}\n\
             Uncovered: {}\n\
             Score:     {}\n",
            self.killed(),
            self.killed_by(KillReason::TestFailure),
            KillReason::TestFailure.label(),
            self.killed_by(KillReason::ArithmeticPanic),
            KillReason::ArithmeticPanic.label(),
            self.killed_by(KillReason::Timeout),
            KillReason::Timeout.label(),
            self.survived(),
            self.uncovered(),
            self.score_line(),
        );
        summary.push_str(&self.listing("Survived mutants:", MutantOutcome::Survived));
        summary.push_str(&self.listing("Uncovered sites:", MutantOutcome::Uncovered));
        summary
    }

    /// A `heading` plus one line per record in `bucket`, or the empty string when
    /// the bucket holds nothing.
    fn listing(&self, heading: &str, bucket: MutantOutcome) -> String {
        let lines: Vec<String> = self
            .records
            .iter()
            .filter(|record| record.bucket() == bucket)
            .map(|record| record.render(&self.file))
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!("{heading}\n{}\n", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::{MutantRecord, MutationReport};
    use crate::outcome::{KillReason, MutantOutcome, MutantResult};
    use crate::site::{Operator, Site};

    const FILE: &str = "src/lib.rs";

    /// A record for a site on `line` carrying `operator`.
    fn record(line: usize, operator: Operator, result: MutantResult) -> MutantRecord {
        MutantRecord::new(&Site::new(operator, 0..1, line, None), result)
    }

    /// Builds a report holding one record per result, all on distinct lines.
    fn report_of(results: &[MutantResult]) -> MutationReport {
        let mut report = MutationReport::new(FILE);
        for (index, result) in results.iter().enumerate() {
            report.record(record(index + 1, Operator::Mul, *result));
        }
        report
    }

    const KILLED: MutantResult = MutantResult::Killed(KillReason::TestFailure);

    #[test]
    fn each_result_lands_in_its_own_bucket() {
        let report = report_of(&[
            KILLED,
            MutantResult::Killed(KillReason::Timeout),
            MutantResult::Survived,
            MutantResult::Uncovered,
            MutantResult::Uncovered,
            MutantResult::Uncovered,
        ]);

        assert_eq!(report.killed(), 2);
        assert_eq!(report.survived(), 1);
        assert_eq!(report.uncovered(), 3);
    }

    /// The counters sum to the record count. They are **structurally** derived —
    /// [`MutationReport`] stores only `file` and `records`, so there is no
    /// parallel tally that could drift; this test pins the partition itself.
    #[test]
    fn the_counters_sum_to_the_record_count() {
        let report = report_of(&[
            KILLED,
            MutantResult::Killed(KillReason::ArithmeticPanic),
            MutantResult::Survived,
            MutantResult::Uncovered,
        ]);

        assert_eq!(
            report.killed() + report.survived() + report.uncovered(),
            report.records().len(),
        );
    }

    #[test]
    fn score_is_killed_over_killed_plus_survived() {
        let report = report_of(&[KILLED, KILLED, KILLED, MutantResult::Survived]);
        assert_eq!(report.score(), Some(0.75));
    }

    #[test]
    fn uncovered_sites_are_excluded_from_the_score() {
        // Same killed/survived tallies, wildly different uncovered counts — the
        // score must not move (uncovered is reported separately, not scored).
        let without = report_of(&[KILLED, MutantResult::Survived]);
        let with = report_of(&[
            KILLED,
            MutantResult::Survived,
            MutantResult::Uncovered,
            MutantResult::Uncovered,
        ]);

        assert_eq!(without.score(), Some(0.5));
        assert_eq!(with.score(), without.score());
    }

    #[test]
    fn score_is_none_when_no_mutant_was_run() {
        // The divide-by-zero edge: an empty run, and a run that was entirely
        // uncovered, both have no scoreable mutant.
        assert_eq!(MutationReport::new(FILE).score(), None);
        assert_eq!(report_of(&[MutantResult::Uncovered]).score(), None);
    }

    #[test]
    fn score_extremes_are_zero_and_one() {
        assert_eq!(report_of(&[KILLED]).score(), Some(1.0));
        assert_eq!(report_of(&[MutantResult::Survived]).score(), Some(0.0));
    }

    /// A8: every kill reason counts toward the SAME bucket, and the breakdown
    /// adds back up to it.
    #[test]
    fn the_kill_breakdown_sums_to_the_killed_bucket() {
        let report = report_of(&[
            KILLED,
            MutantResult::Killed(KillReason::ArithmeticPanic),
            MutantResult::Killed(KillReason::ArithmeticPanic),
            MutantResult::Killed(KillReason::Timeout),
        ]);

        assert_eq!(report.killed(), 4);
        assert_eq!(
            report.summary().lines().next(),
            Some("Killed:    4 (1 test-failure, 2 arithmetic-panic, 1 timeout)"),
        );
    }

    /// The records carry the three things the human asked for — `file:line`,
    /// operator/kind, and outcome — and a panic-kill is distinguishable from an
    /// assertion-kill **without** leaving the Killed bucket.
    #[test]
    fn a_panic_kill_is_distinguishable_from_an_assertion_kill() {
        let mut report = MutationReport::new(FILE);
        report.record(record(4, Operator::Mul, KILLED));
        report.record(record(
            9,
            Operator::AddAssign,
            MutantResult::Killed(KillReason::ArithmeticPanic),
        ));

        let records = report.records();
        assert_ne!(
            records[0].result, records[1].result,
            "the two kills must not be the same record",
        );
        assert_eq!(records[0].bucket(), records[1].bucket());
        assert_eq!(records[0].bucket(), MutantOutcome::Killed);
        assert_eq!(report.killed(), 2, "both are still Killed (A8)");
    }

    /// A6: uncovered sites are **listed** with their locations, not just counted.
    #[test]
    fn the_summary_lists_uncovered_sites_by_location() {
        let mut report = MutationReport::new(FILE);
        report.record(record(11, Operator::AddAssign, MutantResult::Uncovered));
        report.record(record(11, Operator::Mul, MutantResult::Uncovered));

        let summary = report.summary();
        assert!(
            summary.contains("Uncovered sites:\n  src/lib.rs:11 CompoundAssignment `+=`\n  src/lib.rs:11 Arithmetic `*`\n"),
            "{summary}",
        );
    }

    /// A survivor is actionable only with a location.
    #[test]
    fn the_summary_lists_survivors_by_location() {
        let mut report = MutationReport::new(FILE);
        report.record(record(7, Operator::Greater, MutantResult::Survived));

        assert!(
            report
                .summary()
                .contains("Survived mutants:\n  src/lib.rs:7 Comparison `>`\n"),
            "{}",
            report.summary(),
        );
    }

    /// Empty listings are omitted rather than rendered as bare headings.
    #[test]
    fn summary_omits_empty_listings() {
        let summary = report_of(&[KILLED]).summary();
        assert!(!summary.contains("Survived mutants:"), "{summary}");
        assert!(!summary.contains("Uncovered sites:"), "{summary}");
    }

    #[test]
    fn summary_is_stable_and_greppable() {
        let mut report = MutationReport::new(FILE);
        report.record(record(2, Operator::Add, KILLED));
        report.record(record(3, Operator::Sub, KILLED));
        report.record(record(
            4,
            Operator::Mul,
            MutantResult::Killed(KillReason::Timeout),
        ));
        report.record(record(5, Operator::Div, MutantResult::Survived));
        report.record(record(6, Operator::One, MutantResult::Uncovered));

        assert_eq!(
            report.summary(),
            "Killed:    3 (2 test-failure, 0 arithmetic-panic, 1 timeout)\n\
             Survived:  1\n\
             Uncovered: 1\n\
             Score:     75.0% (3 killed of 4 mutants run; 1 uncovered site excluded)\n\
             Survived mutants:\n  \
             src/lib.rs:5 Arithmetic `/`\n\
             Uncovered sites:\n  \
             src/lib.rs:6 Constant `1`\n",
        );
    }

    /// The score line carries its denominator and the uncovered exclusion, so a
    /// partially-covered file cannot be misread as a fully-tested one.
    #[test]
    fn the_score_line_qualifies_the_percentage() {
        let mut report = MutationReport::new(FILE);
        report.record(record(1, Operator::Add, KILLED));
        report.record(record(2, Operator::Sub, MutantResult::Uncovered));

        assert!(
            report.summary().contains(
                "Score:     100.0% (1 killed of 1 mutant run; 1 uncovered site excluded)\n"
            ),
            "{}",
            report.summary(),
        );
    }

    /// Edge case: with nothing excluded the line says nothing about exclusion —
    /// never "0 sites uncovered, excluded".
    #[test]
    fn the_score_line_omits_the_exclusion_when_nothing_was_uncovered() {
        let summary = report_of(&[KILLED, MutantResult::Survived]).summary();

        assert!(
            summary.contains("Score:     50.0% (1 killed of 2 mutants run)\n"),
            "{summary}",
        );
        assert!(!summary.contains("uncovered site"), "{summary}");
    }

    /// Edge case: a zero denominator still reports the exclusion that caused it.
    #[test]
    fn the_score_line_reports_the_exclusion_even_with_no_score() {
        let summary = report_of(&[MutantResult::Uncovered, MutantResult::Uncovered]).summary();

        assert!(
            summary.contains("Score:     n/a (no mutants were run; 2 uncovered sites excluded)\n"),
            "{summary}",
        );
    }

    #[test]
    fn summary_reports_no_score_when_nothing_ran() {
        assert_eq!(
            MutationReport::new(FILE).summary(),
            "Killed:    0 (0 test-failure, 0 arithmetic-panic, 0 timeout)\n\
             Survived:  0\n\
             Uncovered: 0\n\
             Score:     n/a (no mutants were run)\n",
        );
    }
}
