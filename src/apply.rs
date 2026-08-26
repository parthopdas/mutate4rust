//! Byte-span mutant apply + guaranteed restore.
//!
//! This module realizes conflict-resolution **C6** (design.md → the Writer): a
//! mutant is applied by a **byte-span text splice** over the *original* source
//! bytes, and the original file content is **guaranteed** to be restored — even
//! if a test run panics/unwinds mid-flight. The operator token span comes from
//! the scanner ([`crate::site::Site::byte_span`], proven byte-accurate in T4);
//! the file is **never** `syn`-reprinted.
//!
//! Two pieces with a deliberate layer split:
//! - [`splice`] is **pure** (no fs) — a byte-range replacement over a source
//!   string. It stays defensive: a bad span returns `Err` rather than corrupting
//!   bytes or panicking. [`splice_all`] layers one mutant's [`Edit`]s over it.
//! - [`RestoreGuard`] is the **infrastructure/adapter** piece (design.md → the
//!   only layer touching fs): it holds an in-memory copy of the original bytes and
//!   restores them on drop, so an unwinding panic during a test run still leaves
//!   the target file pristine.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::site::Edit;

/// Splices `replacement` over the `span` byte range of `original`, returning
/// `original[..span.start] + replacement + original[span.end..]`.
///
/// This is a **pure** byte-range replacement — no `syn`-reprint, no fs. It is the
/// mechanism behind C6: the operator token span from the scanner is replaced by
/// the mutated token text, leaving every other byte untouched.
///
/// # Errors
///
/// Returns an error — never panics — when the span is invalid: `span.start`
/// exceeds `span.end`, `span.end` exceeds `original.len()`, or either end does
/// not fall on a UTF-8 char boundary. Our own scanner spans are always valid;
/// this validation is defensive so a bad span can never silently corrupt bytes.
pub fn splice(original: &str, span: &Range<usize>, replacement: &str) -> Result<String> {
    ensure!(
        span.start <= span.end,
        "invalid splice span: start {} exceeds end {}",
        span.start,
        span.end,
    );
    ensure!(
        span.end <= original.len(),
        "invalid splice span: end {} exceeds source length {}",
        span.end,
        original.len(),
    );
    ensure!(
        original.is_char_boundary(span.start),
        "invalid splice span: start {} is not a UTF-8 char boundary",
        span.start,
    );
    ensure!(
        original.is_char_boundary(span.end),
        "invalid splice span: end {} is not a UTF-8 char boundary",
        span.end,
    );

    let mut spliced =
        String::with_capacity(original.len() - (span.end - span.start) + replacement.len());
    spliced.push_str(&original[..span.start]);
    spliced.push_str(replacement);
    spliced.push_str(&original[span.end..]);
    Ok(spliced)
}

/// Applies every edit of one mutant over `original`, in **descending start
/// order**.
///
/// Order is the whole point: an edit changes the length of the text after it, so
/// applying a lower edit first invalidates every higher offset. Applying from the
/// end backwards leaves the not-yet-applied spans untouched, so each edit's span
/// still means what the scanner said it meant. The caller may pass edits in any
/// order.
///
/// Edits **within one mutant must not overlap** — overlapping ones would splice
/// over each other's text and silently produce bytes no site described, so they
/// are rejected rather than applied. Spans of *different* mutants freely overlap
/// (see [`crate::scanner`]'s module header); each is applied to pristine source
/// on its own.
///
/// # Errors
///
/// Returns an error — never panics — if two edits overlap, or if any span is
/// invalid in the sense [`splice`] documents.
pub fn splice_all(original: &str, edits: &[Edit]) -> Result<String> {
    let mut ordered: Vec<&Edit> = edits.iter().collect();
    ordered.sort_by_key(|edit| std::cmp::Reverse(edit.span.start));

    let mut spliced = original.to_owned();
    let mut lowest_applied = usize::MAX;
    for edit in ordered {
        ensure!(
            edit.span.end <= lowest_applied,
            "overlapping edits within one mutant: {:?} runs into {lowest_applied}",
            edit.span,
        );
        spliced = splice(&spliced, &edit.span, &edit.replacement)?;
        lowest_applied = edit.span.start;
    }
    Ok(spliced)
}

/// A scope guard that overwrites a target file with mutated content and
/// **guarantees** the original bytes are restored.
///
/// On construction the original file is read into an in-memory `String`. The
/// caller applies a mutant with [`write_mutant`](RestoreGuard::write_mutant) and,
/// on the success path, calls [`restore`](RestoreGuard::restore) explicitly so a
/// real I/O error surfaces. If the caller unwinds (a panicking test run) or
/// returns early before restoring, [`Drop`] restores the original bytes as a
/// best-effort safety net.
///
/// The guard writes bytes **verbatim** — it never normalizes or reformats, so LF
/// line endings are preserved exactly.
pub struct RestoreGuard {
    path: PathBuf,
    original: String,
}

impl RestoreGuard {
    /// Reads `path` into memory as the original source and returns a guard bound
    /// to it.
    ///
    /// # Errors
    ///
    /// Returns an error if the target file cannot be read.
    pub fn new(path: &Path) -> Result<Self> {
        let original = fs::read_to_string(path)
            .with_context(|| format!("failed to read target file `{}`", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            original,
        })
    }

    /// Lends the in-memory original source, so the caller can [`splice`] against
    /// it without re-reading the disk.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Overwrites the target file with `mutated`.
    ///
    /// Bytes are written verbatim (LF preserved) — no normalization.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn write_mutant(&self, mutated: &str) -> Result<()> {
        fs::write(&self.path, mutated)
            .with_context(|| format!("failed to write mutant to `{}`", self.path.display()))
    }

    /// Rewrites the original bytes back to the target file.
    ///
    /// Idempotent — safe to call repeatedly, and safe to call even after [`Drop`]
    /// has already restored. Prefer this on the success path so a genuine I/O
    /// failure surfaces (Drop deliberately swallows write errors).
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn restore(&self) -> Result<()> {
        fs::write(&self.path, &self.original)
            .with_context(|| format!("failed to restore original to `{}`", self.path.display()))
    }
}

impl Drop for RestoreGuard {
    /// Best-effort restore of the original bytes, so an unwinding panic during a
    /// test run still leaves the target file pristine. Drop must not panic, so a
    /// write error here is deliberately swallowed — explicit
    /// [`restore`](RestoreGuard::restore) is preferred on the success path where
    /// real I/O errors should surface.
    fn drop(&mut self) {
        let _ = fs::write(&self.path, &self.original);
    }
}

#[cfg(test)]
mod tests {
    use super::{RestoreGuard, splice, splice_all};
    use crate::site::Edit;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// Descending application order is what makes multi-edit mutants correct: an
    /// edit changes the length of everything after it, so applying the *lower*
    /// edit first would shift the higher span off its token. The edits are handed
    /// over in ascending order here on purpose — `splice_all` orders them itself.
    ///
    /// The replacements deliberately differ in length from what they replace, so
    /// an ascending application does not merely happen to work.
    #[test]
    fn splice_all_applies_every_edit_regardless_of_input_order() {
        let original = "a + b - c\n";
        let ascending = [
            Edit::new(2..3, "MINUS".to_owned()),
            Edit::new(6..7, "+".to_owned()),
        ];
        let descending = [ascending[1].clone(), ascending[0].clone()];

        assert_eq!(
            splice_all(original, &ascending).expect("valid edits"),
            "a MINUS b + c\n",
        );
        assert_eq!(
            splice_all(original, &descending).expect("valid edits"),
            "a MINUS b + c\n",
        );
    }

    /// Two edits of **one** mutant must not overlap — they would splice over each
    /// other's text and yield bytes no site described. Spans of *different*
    /// mutants may overlap freely; each is applied to pristine source alone.
    #[test]
    fn splice_all_rejects_overlapping_edits() {
        let original = "a + b - c\n";
        let overlapping = [
            Edit::new(0..5, "x".to_owned()),
            Edit::new(4..9, "y".to_owned()),
        ];

        assert!(splice_all(original, &overlapping).is_err());
        // Abutting edits are not overlapping: `..5` ends where `5..` begins.
        let abutting = [
            Edit::new(0..5, "x".to_owned()),
            Edit::new(5..9, "y".to_owned()),
        ];
        assert_eq!(
            splice_all(original, &abutting).expect("valid edits"),
            "xy\n"
        );
    }

    /// A swap is two edits trading text; both land, and nothing between them
    /// moves.
    #[test]
    fn splice_all_swaps_two_ranges() {
        let original = "match x { 2 => 7, _ => 9 }\n";
        let first = original.find('7').expect("first body");
        let second = original.find('9').expect("second body");
        let edits = [
            Edit::new(first..first + 1, "9".to_owned()),
            Edit::new(second..second + 1, "7".to_owned()),
        ];

        assert_eq!(
            splice_all(original, &edits).expect("valid edits"),
            "match x { 2 => 9, _ => 7 }\n",
        );
    }

    /// An empty edit list leaves the source untouched, and one invalid span still
    /// errors rather than corrupting bytes.
    #[test]
    fn splice_all_handles_the_degenerate_cases() {
        let original = "a + b\n";
        assert_eq!(splice_all(original, &[]).expect("no edits"), original);
        assert!(splice_all(original, &[Edit::new(0..99, "x".to_owned())]).is_err());
    }

    #[test]
    fn splice_replaces_token_in_the_middle() {
        let original = "a + b";
        // The `+` operator token is the single byte at index 2.
        let result = splice(original, &(2..3), "-").expect("valid span");
        assert_eq!(result, "a - b");
    }

    #[test]
    fn splice_preserves_multibyte_prefix_byte_intact() {
        // Multibyte UTF-8 before the target token (T4 idea): the `+` must be
        // replaced by `-` with every prefix byte surviving unchanged.
        let original = "café ☕ dé a + b";
        let plus = original.find('+').expect("token present");
        let result = splice(original, &(plus..plus + 1), "-").expect("valid span");
        assert_eq!(result, "café ☕ dé a - b");
    }

    #[test]
    fn splice_rejects_end_past_length() {
        let original = "a + b";
        assert!(splice(original, &(0..original.len() + 1), "-").is_err());
    }

    #[test]
    fn splice_rejects_start_after_end() {
        let original = "a + b";
        // Construct the reversed range via struct literal to exercise the
        // `start > end` guard without tripping clippy's empty-range lint.
        let span = std::ops::Range { start: 3, end: 2 };
        assert!(splice(original, &span, "-").is_err());
    }

    #[test]
    fn splice_rejects_non_char_boundary() {
        // `é` is two bytes; splitting inside it is not a char boundary.
        let original = "é";
        assert!(splice(original, &(0..1), "e").is_err());
    }

    #[test]
    fn restore_guard_round_trip() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("target.rs");
        let original = "fn f(a: i32, b: i32) -> i32 { a + b }\n";
        std::fs::write(&path, original).expect("seed original");

        let guard = RestoreGuard::new(&path).expect("read original");
        let mutated = splice(guard.original(), &(32..33), "-").expect("valid span");
        guard.write_mutant(&mutated).expect("write mutant");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read mutated"),
            "fn f(a: i32, b: i32) -> i32 { a - b }\n",
        );

        guard.restore().expect("restore");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read restored"),
            original,
        );
    }

    #[test]
    fn restore_on_panic_leaves_file_pristine() {
        // The core T7 guarantee: if a test run panics while the guard is in scope,
        // Drop must restore the original bytes on unwind.
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("target.rs");
        let original = "let x = a + b;\n";
        std::fs::write(&path, original).expect("seed original");

        let caught = catch_unwind(AssertUnwindSafe(|| {
            let guard = RestoreGuard::new(&path).expect("read original");
            let mutated = splice(guard.original(), &(10..11), "-").expect("valid span");
            guard.write_mutant(&mutated).expect("write mutant");
            // The file on disk is the mutant at the moment of panic.
            assert_eq!(
                std::fs::read_to_string(&path).expect("read mutated"),
                "let x = a - b;\n",
            );
            panic!("simulated test-run panic while mutant is applied");
        }));

        assert!(caught.is_err(), "the closure must have panicked");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read after panic"),
            original,
            "Drop must restore the original bytes on unwind",
        );
    }

    #[test]
    fn restore_on_normal_drop() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("target.rs");
        let original = "let x = a + b;\n";
        std::fs::write(&path, original).expect("seed original");

        {
            let guard = RestoreGuard::new(&path).expect("read original");
            let mutated = splice(guard.original(), &(10..11), "-").expect("valid span");
            guard.write_mutant(&mutated).expect("write mutant");
            // No explicit restore — Drop at end of scope must restore.
        }

        assert_eq!(
            std::fs::read_to_string(&path).expect("read after drop"),
            original,
        );
    }

    #[test]
    fn write_mutant_preserves_lf() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("target.rs");
        let original = "line1\nline2\n";
        std::fs::write(&path, original).expect("seed original");

        let guard = RestoreGuard::new(&path).expect("read original");
        let mutated = "line1\nline2 changed\nline3\n";
        guard.write_mutant(mutated).expect("write mutant");

        // Read raw bytes: no CRLF must have been introduced.
        let bytes = std::fs::read(&path).expect("read raw bytes");
        assert!(
            !bytes.windows(2).any(|w| w == b"\r\n"),
            "guard must not normalize LF to CRLF",
        );
        assert_eq!(bytes, mutated.as_bytes());
    }
}
