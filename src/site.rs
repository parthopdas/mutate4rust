//! Pure domain model for a discovered mutation site.
//!
//! This module is **Core/domain** (design.md → inward dependency flow): it holds
//! no fs/process/argv/clap dependencies and never `use`s the runner, coverage, or
//! CLI layers. It describes only *what* a mutation site is — its taxonomy class,
//! byte span, line, and enclosing function — leaving the actual byte-mutation
//! mapping to the operators layer (T9) and the AST walk to [`crate::scanner`].

use std::ops::Range;

/// Taxonomy category of a mutation site.
///
/// The discovered classes are the feature file's **universal + arithmetic-parity**
/// set (S3) plus the **arithmetic idiomatic completions** (S4: `/`, `%`, and the
/// compound-assignment operators). Each class maps to one or more concrete
/// [`Operator`]s. Bitwise and shift operators remain out of scope (S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// Binary arithmetic operator — `+`, `-`, `*` (parity) and `/`, `%` (S4).
    Arithmetic,
    /// Ordering comparison — `>`, `>=`, `<`, `<=`.
    Comparison,
    /// Equality comparison — `==`, `!=`.
    Equality,
    /// Boolean literal — `true`, `false`.
    BooleanLiteral,
    /// Short-circuit logical operator — `&&`, `||`.
    Logical,
    /// Integer constant `0` or `1`.
    Constant,
    /// Compound-assignment operator — `+=`, `-=`, `*=`, `/=`, `%=`.
    CompoundAssignment,
}

/// The exact operator or token found at a site.
///
/// This is the precise classification the mutation mapping (T9) needs to compute
/// the mutated bytes; the coarser [`SiteKind`] is derived from it via
/// [`Operator::kind`]. T4 does **not** perform any mutation — it only records
/// which operator/token was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `>`
    Greater,
    /// `>=`
    GreaterEqual,
    /// `<`
    Less,
    /// `<=`
    LessEqual,
    /// `==`
    Equal,
    /// `!=`
    NotEqual,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `true`
    True,
    /// `false`
    False,
    /// integer literal `0`
    Zero,
    /// integer literal `1`
    One,
    /// `+=`
    AddAssign,
    /// `-=`
    SubAssign,
    /// `*=`
    MulAssign,
    /// `/=`
    DivAssign,
    /// `%=`
    RemAssign,
}

impl Operator {
    /// The taxonomy [`SiteKind`] this operator belongs to.
    #[must_use]
    pub fn kind(self) -> SiteKind {
        match self {
            Operator::Add | Operator::Sub | Operator::Mul | Operator::Div | Operator::Rem => {
                SiteKind::Arithmetic
            }
            Operator::Greater | Operator::GreaterEqual | Operator::Less | Operator::LessEqual => {
                SiteKind::Comparison
            }
            Operator::Equal | Operator::NotEqual => SiteKind::Equality,
            Operator::And | Operator::Or => SiteKind::Logical,
            Operator::True | Operator::False => SiteKind::BooleanLiteral,
            Operator::Zero | Operator::One => SiteKind::Constant,
            Operator::AddAssign
            | Operator::SubAssign
            | Operator::MulAssign
            | Operator::DivAssign
            | Operator::RemAssign => SiteKind::CompoundAssignment,
        }
    }
}

/// A discovered mutation site in a single source file.
///
/// Byte offsets in [`byte_span`](Site::byte_span) are **relative to the start of
/// the scanned source string** (proc-macro2 fallback offsets), so
/// `&source[site.byte_span.start..site.byte_span.end]` yields exactly the
/// operator/token text. Those offsets are the input to the O1 byte-span-splice
/// mutation strategy (design C6, risk R2) applied in T7/T9 — the file is never
/// `syn`-reprinted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// Taxonomy category. Always equals `operator.kind()` (see [`Site::new`]).
    pub kind: SiteKind,
    /// The exact operator/token found — precise enough for T9 to mutate.
    pub operator: Operator,
    /// Byte range of the operator/token within the source string (`start..end`).
    pub byte_span: Range<usize>,
    /// 1-based line number of the site.
    pub line: usize,
    /// Fully-qualified path of the enclosing function (e.g. `Foo::bar`,
    /// `outer::inner`, `m::inner`), or `None` when the site is not inside any
    /// function body — e.g. a module-level or associated `const`. Used to group
    /// sites by function for S7 differential hashing.
    pub function_id: Option<String>,
}

impl Site {
    /// Builds a site, deriving [`kind`](Site::kind) from `operator` so the two can
    /// never disagree — `operator` is the single source of truth.
    #[must_use]
    pub fn new(
        operator: Operator,
        byte_span: Range<usize>,
        line: usize,
        function_id: Option<String>,
    ) -> Self {
        Self {
            kind: operator.kind(),
            operator,
            byte_span,
            line,
            function_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Operator, SiteKind};

    /// Every operator maps to the taxonomy class the feature file assigns it.
    #[test]
    fn operator_kind_matches_taxonomy() {
        assert_eq!(Operator::Add.kind(), SiteKind::Arithmetic);
        assert_eq!(Operator::Sub.kind(), SiteKind::Arithmetic);
        assert_eq!(Operator::Mul.kind(), SiteKind::Arithmetic);
        assert_eq!(Operator::Div.kind(), SiteKind::Arithmetic);
        assert_eq!(Operator::Rem.kind(), SiteKind::Arithmetic);
        assert_eq!(Operator::Greater.kind(), SiteKind::Comparison);
        assert_eq!(Operator::GreaterEqual.kind(), SiteKind::Comparison);
        assert_eq!(Operator::Less.kind(), SiteKind::Comparison);
        assert_eq!(Operator::LessEqual.kind(), SiteKind::Comparison);
        assert_eq!(Operator::Equal.kind(), SiteKind::Equality);
        assert_eq!(Operator::NotEqual.kind(), SiteKind::Equality);
        assert_eq!(Operator::And.kind(), SiteKind::Logical);
        assert_eq!(Operator::Or.kind(), SiteKind::Logical);
        assert_eq!(Operator::True.kind(), SiteKind::BooleanLiteral);
        assert_eq!(Operator::False.kind(), SiteKind::BooleanLiteral);
        assert_eq!(Operator::Zero.kind(), SiteKind::Constant);
        assert_eq!(Operator::One.kind(), SiteKind::Constant);
        assert_eq!(Operator::AddAssign.kind(), SiteKind::CompoundAssignment);
        assert_eq!(Operator::SubAssign.kind(), SiteKind::CompoundAssignment);
        assert_eq!(Operator::MulAssign.kind(), SiteKind::CompoundAssignment);
        assert_eq!(Operator::DivAssign.kind(), SiteKind::CompoundAssignment);
        assert_eq!(Operator::RemAssign.kind(), SiteKind::CompoundAssignment);
    }
}
