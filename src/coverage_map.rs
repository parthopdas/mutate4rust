//! Pure line-coverage model (**Core/domain**, design.md → inward dependency flow).
//!
//! The coverage backend (`cargo-llvm-cov`) reports **region** coverage, but the
//! mutation pipeline reasons in **lines** — a [`crate::site::Site`] carries a
//! 1-based `line`, not a region. Conflict C8 resolves that by mapping regions
//! onto lines; this module is where that mapped value lives, deliberately free of
//! fs/process/serde so the query is unit-testable in isolation. Parsing an
//! llvm-cov export into one of these is the boundary's job
//! ([`crate::coverage`]).
//!
//! # Coverage rule (C8/R5)
//!
//! > A line is **covered** when at least one region whose inclusive line range
//! > `start_line..=end_line` contains it has an execution `count > 0`.
//!
//! Two properties make this the right rule:
//!
//! - **Regions span lines.** A statement is frequently *inside* a multi-line
//!   region rather than the start of one — in a real capture, the body of a `for`
//!   loop is a single region `[3, 25] .. [5, 6]`, so the `total += value;` on line
//!   4 has no region of its own. Keying on region *start* lines (the naive
//!   mapping R5 warns about) would mis-mark that line uncovered and silently skip
//!   every site on it.
//! - **Any covered region wins.** The rule is monotone: a region can only ever
//!   *add* coverage, never remove it. A line carrying both an executed and an
//!   unexecuted region (a short-circuited `&&`, an untaken `match` arm) is
//!   covered — the mutation *is* reachable, which is what gating needs to know.
//!
//! This is byte-for-byte the semantics of upstream mutate4go's
//! `coverage.Covered` (`line >= StartLine && line <= EndLine && Count > 0`), so
//! the Go profile's line segments and llvm-cov's regions land on the same answer.

/// One contiguous coverage region, reduced to the fields line mapping needs.
///
/// Line numbers are **1-based and inclusive** at both ends, matching both
/// llvm-cov's export and [`crate::site::Site::line`]. Columns are deliberately
/// dropped: coverage is queried per line, so sub-line precision would only
/// invite disagreement between two sites on the same line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// First line of the region (1-based, inclusive).
    pub start_line: usize,
    /// Last line of the region (1-based, inclusive).
    pub end_line: usize,
    /// How many times the region executed; `0` means not covered.
    pub count: u64,
}

/// Line coverage for a **single source file**, queried by line number.
///
/// An empty map (the [`Default`]) reports every line uncovered — the correct
/// answer when the profile holds no data for the file at all, and the same
/// behaviour as upstream's `Covered` against an absent profile entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageMap {
    regions: Vec<Region>,
}

impl CoverageMap {
    /// Builds a map from the regions belonging to one file.
    #[must_use]
    pub fn new(regions: Vec<Region>) -> Self {
        Self { regions }
    }

    /// Whether `line` (1-based) is covered, per the module's coverage rule.
    ///
    /// Every site on a given line therefore resolves **identically** — the line
    /// is the whole key, so a multi-site line can never split into covered and
    /// uncovered sites.
    #[must_use]
    pub fn is_line_covered(&self, line: usize) -> bool {
        self.regions
            .iter()
            .any(|region| region.count > 0 && region.start_line <= line && line <= region.end_line)
    }

    /// Whether the map holds no regions at all — i.e. the profile said nothing
    /// about this file, so every line reads as uncovered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{CoverageMap, Region};

    /// Builds a region spanning `start..=end` with execution count `count`.
    fn region(start: usize, end: usize, count: u64) -> Region {
        Region {
            start_line: start,
            end_line: end,
            count,
        }
    }

    #[test]
    fn an_empty_map_covers_nothing() {
        let map = CoverageMap::default();
        assert!(map.is_empty());
        assert!(!map.is_line_covered(1));
        assert!(!map.is_line_covered(usize::MAX));
    }

    #[test]
    fn an_executed_region_covers_its_own_line() {
        let map = CoverageMap::new(vec![region(7, 7, 1)]);
        assert!(map.is_line_covered(7));
    }

    #[test]
    fn a_zero_count_region_covers_nothing() {
        let map = CoverageMap::new(vec![region(9, 13, 0)]);
        assert!(!map.is_line_covered(9));
        assert!(!map.is_line_covered(11));
        assert!(!map.is_line_covered(13));
    }

    #[test]
    fn a_multi_line_region_covers_its_interior_lines() {
        // The R5 case: a loop body is ONE region spanning lines 3..=5, so the
        // statement on line 4 has no region starting on it. A start-line-keyed
        // mapping would call line 4 uncovered.
        let map = CoverageMap::new(vec![region(3, 5, 3)]);
        assert!(map.is_line_covered(3));
        assert!(map.is_line_covered(4), "interior line must be covered");
        assert!(map.is_line_covered(5));
    }

    #[test]
    fn region_bounds_are_inclusive_and_lines_outside_them_are_uncovered() {
        let map = CoverageMap::new(vec![region(10, 12, 2)]);
        assert!(!map.is_line_covered(9));
        assert!(map.is_line_covered(10));
        assert!(map.is_line_covered(12));
        assert!(!map.is_line_covered(13));
    }

    #[test]
    fn a_line_with_mixed_regions_is_covered_when_any_region_executed() {
        // Line 20 carries one executed and one unexecuted region (e.g. a
        // short-circuited operand). Any-covered-wins: the line is covered.
        let map = CoverageMap::new(vec![region(20, 20, 0), region(20, 20, 4)]);
        assert!(map.is_line_covered(20));
    }

    #[test]
    fn mixed_regions_are_order_independent() {
        let covered_first = CoverageMap::new(vec![region(20, 20, 4), region(20, 20, 0)]);
        let uncovered_first = CoverageMap::new(vec![region(20, 20, 0), region(20, 20, 4)]);
        assert_eq!(
            covered_first.is_line_covered(20),
            uncovered_first.is_line_covered(20)
        );
    }

    #[test]
    fn covered_and_uncovered_regions_of_one_file_do_not_bleed_into_each_other() {
        // A covered function (3..=5) and an uncovered one (9..=13) in the same
        // map: neither leaks coverage into the other's lines.
        let map = CoverageMap::new(vec![region(3, 5, 3), region(9, 13, 0)]);
        assert!(map.is_line_covered(4));
        assert!(!map.is_line_covered(11));
        assert!(!map.is_line_covered(7), "the gap between them is uncovered");
    }
}
