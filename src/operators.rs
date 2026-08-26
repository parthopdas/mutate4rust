//! Operator mappings: a discovered site's original token → its mutated token
//! (Core/domain).
//!
//! This module is **pure** (design.md → inward dependency flow): it takes token
//! text and returns token text, with no fs/process/argv/clap dependency and no
//! knowledge of the runner, coverage, or CLI layers. The caller slices the token
//! out of the original source using [`crate::site::Site::byte_span`] and splices
//! the returned replacement back with [`crate::apply::splice`].
//!
//! # Scope — the S3 universal + arithmetic-**parity** set, the S4 arithmetic
//! idiomatic completions, and the S6 token-level Rust operators
//!
//! | Class | Mapping | Source |
//! |-------|---------|--------|
//! | Arithmetic | `+ → -`; `- → +`; `* → /` | parity |
//! | Comparison | `> → >=`; `>= → >`; `< → <=`; `<= → <` | parity |
//! | Equality | `== → !=`; `!= → ==` | parity |
//! | Boolean literal | `true → false`; `false → true` | parity |
//! | Logical | `&& → \|\|`; `\|\| → &&` | parity |
//! | Constant | `0 → 1`; `1 → 0` | parity |
//! | Arithmetic (division) | `/ → *` | idiomatic (S4) |
//! | Arithmetic (remainder) | `% → *` | idiomatic (S4) |
//! | Compound assignment | `+= → -=`; `-= → +=`; `*= → /=`; `/= → *=`; `%= → *=` | idiomatic (S4) |
//! | Float constant | `0.0 → 1.0`; `1.0 → 0.0` | idiomatic (S6) |
//! | Bitwise | `& → \|`; `\| → &`; `^ → &` | idiomatic (S6) |
//! | Shift | `<< → >>`; `>> → <<` | idiomatic (S6) |
//! | Predicate method | `is_some → is_none`; `is_none → is_some`; `is_ok → is_err`; `is_err → is_ok` | idiomatic (S6) |
//! | `Option`/`Result` | `Some(x) → None`; `Ok → Err` | idiomatic (S6) |
//! | `unwrap` | `unwrap → unwrap_or_default` | idiomatic (S6) |
//! | `expect` | `expect(_) → unwrap_or_default()` | idiomatic (S6) |
//! | `?` operator | `? → .unwrap()` | idiomatic (S6) |
//! | `match`-arm guard | `if <guard> → ` (dropped) | idiomatic (S6) |
//! | `match`-arm bodies | two arms trade bodies | idiomatic (S6) |
//!
//! The structural S6 rows are token-**independent**: each is one span the
//! scanner chose so that a single replacement suffices — the whole `Some(x)`
//! call, the `Ok` identifier alone (so the bound value survives), the `unwrap`
//! method identifier, the whole `expect(msg)` call (so its argument goes with
//! it), the `?` token, and the arm's `if <guard>`. Their
//! preconditions (`T: Default`, a compatible error type, a permitting `Try`
//! type) are **undecidable without type information**, so unlike the float rows
//! they cannot be precondition-gated at emission; a mutant that does not compile
//! is scored `Killed` with [`crate::outcome::KillReason::CompileError`] (A8/R4).
//!
//! The **arm body swap** is the one row that is not a mapping at all: its
//! replacement text is source at *another* location in the file, which
//! [`replacement`] by construction cannot read. It is expressed as two
//! [`Edit`]s by [`edits`] — see [`Edit`] for why that, and not span
//! disjointness, is what the multi-edit model is for.
//!
//! The three parity arithmetic mappings are **byte-identical to mutate4go** and
//! must stay that way: S4/S6 only *add* the operators upstream never mutated. The
//! resulting asymmetry is deliberate — `* → /` (parity) and `/ → *` (idiomatic)
//! coexist, while `% → *`, `%= → *=` and `^ → &` are one-way.
//!
//! # Integer literals keep their radix and suffix
//!
//! A constant site's byte span covers the **whole literal token**, so a naive
//! splice of `"0"` over `1u8` would drop the suffix and break type inference, and
//! over `0x1` would silently change the radix in the diff. [`replacement`]
//! therefore rewrites only the digits, preserving any `0x`/`0o`/`0b` radix prefix
//! and any integer suffix: `1u8 → 0u8`, `0x1 → 0x0`, `0b0 → 0b1`.
//!
//! # Float literals: which forms are rewritten, and which are left alone
//!
//! Floats are a **separate path** from the integer one (a `syn::LitFloat`
//! scanner arm and a float-aware mapping); `mutate_int_literal` is deliberately
//! not stretched to cover them.
//!
//! A float site is only ever discovered — and only ever rewritten — when, after
//! removing `_` separators and any `f32`/`f64` suffix, the literal's digits are a
//! **plain decimal fraction** `digits "." digits` whose value is exactly `0` or
//! `1`. Such a literal is canonicalized to `0.0`/`1.0` with its suffix preserved
//! verbatim: `0.0 → 1.0`, `1.0f32 → 0.0f32`, `0.0_f32 → 1.0f32` (the separator
//! before a suffix is dropped, exactly as on the integer path), `1.000 → 0.0`.
//!
//! Every other spelling is **left alone — no site is emitted at all** (the
//! taxonomy's precondition-gated emission), because rewriting it would mean
//! silently re-spelling the literal in a form the author did not write:
//!
//! * exponent forms — `1e0`, `0.0e0`, `1E3`;
//! * suffix-only forms with no decimal point — `0f64`, `1f32`;
//! * trailing-dot forms — `0.`, `1.`.

use anyhow::{Context, Result, bail, ensure};

use crate::site::{Edit, Operator, Site};

/// Every Rust integer-literal suffix, longest first so a strip is unambiguous.
const INT_SUFFIXES: [&str; 12] = [
    "usize", "isize", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
];

/// Whether `suffix` is a Rust **integer** literal suffix (the empty suffix
/// counts as one).
///
/// `0f64` parses as a `syn::LitInt` carrying the suffix `f64` — a float literal
/// in an integer's clothing. [`mutate_int_literal`] cannot rewrite it (the
/// suffix is not strippable as an integer one) and the point-less float
/// spelling is deliberately out of scope, so [`crate::scanner`] uses this to
/// keep it out of the constant class rather than emit a site whose mapping
/// would fail mid-run.
pub(crate) fn is_integer_suffix(suffix: &str) -> bool {
    suffix.is_empty() || INT_SUFFIXES.contains(&suffix)
}

/// Every Rust float-literal suffix.
const FLOAT_SUFFIXES: [&str; 2] = ["f32", "f64"];

/// Every Rust integer-literal radix prefix.
const RADIX_PREFIXES: [&str; 6] = ["0x", "0X", "0o", "0O", "0b", "0B"];

/// Every byte-range replacement making up the mutant for `site`, over the
/// pristine `original`.
///
/// The **single** place a site becomes edits. Two shapes:
/// - the **single-token fast path** — one [`Edit`] over the site's own span,
///   whose text comes from [`replacement`]. Every operator but one takes it, and
///   deliberately so: routing them through the multi-edit path would buy nothing
///   and hide the mapping.
/// - the **swap path** — the two spans of a [`Site::swap_span`] site trade their
///   source text. This exists because [`replacement`] is a function of
///   `(Operator, token)` and cannot read source elsewhere in the file; see
///   [`Edit`].
///
/// # Errors
///
/// Returns an error if either span falls outside `original` or off a UTF-8 char
/// boundary, or if the operator's mapping rejects the token (an internal
/// invariant violation — our own scanner never emits such a site).
pub(crate) fn edits(original: &str, site: &Site) -> Result<Vec<Edit>> {
    let token = span_text(original, &site.byte_span, site.line)?;
    let Some(swap_span) = &site.swap_span else {
        return Ok(vec![Edit::new(
            site.byte_span.clone(),
            replacement(site.operator, token)?,
        )]);
    };
    let other = span_text(original, swap_span, site.line)?;
    // **Textually identical bodies are a known equivalent-mutant source, deferred
    // to T13c′.** When the two spans hold the same text, these two edits reproduce
    // the original byte-for-byte: a provably-equivalent `Survived`, polluting the
    // one actionable bucket with an entry no test can ever kill. **In this
    // codebase**, 30 of 215 swap sites (~14%) trade identical bodies — the
    // `&node.attrs` fan-outs in the `*_attrs` helpers and the `"*"`/`"*="`/`"&"`
    // arms in `replacement`. Read that as a census of *these* sources, which are
    // unrepresentative in exactly the measured dimension (wide dispatch `match`es
    // with byte-identical arms), never as a general rate.
    //
    // Two identical bodies are decidable **syntactically**, so the S6 precondition
    // rule applies: the site is suppressed at emission, in the scanner — that is
    // T13c′. (A second decidable class, a swapped body referencing a pattern-bound
    // identifier, is being *measured* first via `KillReason::CompileError` rather
    // than gated.)
    // Today the only detection is one `==` *here*, at the apply layer, where both
    // strings are in hand. What stays A5's is the **bucketing policy** for
    // whatever is not suppressed: the three buckets are parity-locked, so whether
    // such a mutant is skipped outright or reported `Survived` with a note is the
    // human's call.
    Ok(vec![
        Edit::new(site.byte_span.clone(), other.to_owned()),
        Edit::new(swap_span.clone(), token.to_owned()),
    ])
}

/// The source text under `span`, or a located error if the span is not a valid
/// slice of `original`.
fn span_text<'a>(original: &'a str, span: &std::ops::Range<usize>, line: usize) -> Result<&'a str> {
    original
        .get(span.clone())
        .with_context(|| format!("mutation site at line {line} has a byte span outside the source"))
}

/// The mutated token text for `operator`, given the site's original token text.
///
/// `token` is only consulted for the literal operators
/// ([`Operator::Zero`]/[`Operator::One`], where the radix prefix and suffix must
/// be preserved, and [`Operator::FloatZero`]/[`Operator::FloatOne`], where the
/// `f32`/`f64` suffix must be); every other mapping is a fixed token-for-token
/// swap. For a predicate method the token is the bare method name — the site's
/// span covers the identifier only, never the receiver or the `()`.
///
/// # Errors
///
/// Returns an error — never panics — if a constant site's token is not a
/// well-formed Rust integer/float literal of the form the operator was
/// discovered for, or if its value is not the constant the operator was
/// discovered for (neither could have come from our own scanner).
pub(crate) fn replacement(operator: Operator, token: &str) -> Result<String> {
    let mapped = match operator {
        Operator::Add => "-",
        Operator::Sub => "+",
        Operator::Mul => "/",
        Operator::Div => "*",
        Operator::Rem => "*",
        Operator::Greater => ">=",
        Operator::GreaterEqual => ">",
        Operator::Less => "<=",
        Operator::LessEqual => "<",
        Operator::Equal => "!=",
        Operator::NotEqual => "==",
        Operator::And => "||",
        Operator::Or => "&&",
        Operator::True => "false",
        Operator::False => "true",
        Operator::AddAssign => "-=",
        Operator::SubAssign => "+=",
        Operator::MulAssign => "/=",
        Operator::DivAssign => "*=",
        Operator::RemAssign => "*=",
        Operator::BitAnd => "|",
        Operator::BitOr => "&",
        Operator::BitXor => "&",
        Operator::Shl => ">>",
        Operator::Shr => "<<",
        Operator::IsSome => "is_none",
        Operator::IsNone => "is_some",
        Operator::IsOk => "is_err",
        Operator::IsErr => "is_ok",
        Operator::SomeCall => "None",
        Operator::OkCall => "Err",
        Operator::Unwrap => "unwrap_or_default",
        // The span covers `expect(msg)` whole, so the message argument is
        // replaced along with the method name.
        Operator::Expect => "unwrap_or_default()",
        Operator::Try => ".unwrap()",
        // The span covers `if <guard>`; dropping it leaves the arm's pattern and
        // body untouched.
        Operator::ArmGuard => "",
        // The one operator with no token-for-token replacement: its text comes
        // from another location in the file, so it is expressible only as the two
        // edits [`edits`] builds. Reaching here means a caller bypassed `edits`.
        Operator::ArmBodySwap => {
            bail!("`match` arm bodies are swapped as two edits, not mapped from a token")
        }
        Operator::Zero => return mutate_int_literal(token, 0, '1'),
        Operator::One => return mutate_int_literal(token, 1, '0'),
        Operator::FloatZero => return mutate_float_literal(token, 0, "1.0"),
        Operator::FloatOne => return mutate_float_literal(token, 1, "0.0"),
    };
    Ok(mapped.to_owned())
}

/// Rewrites an integer literal's digits to `digit`, keeping its radix prefix and
/// suffix so the mutant still compiles with the original type and base.
///
/// The token is validated first: it must be a well-formed Rust integer literal
/// **and** its value must be `expected` — the constant the operator was
/// discovered for. Anything else is a token our scanner never emitted, so it
/// errors rather than silently normalizing into a valid but unrelated literal
/// (`"garbage"` must not become `"0"`).
fn mutate_int_literal(token: &str, expected: u128, digit: char) -> Result<String> {
    let (body, suffix) = split_suffix(token);
    let prefix = RADIX_PREFIXES
        .iter()
        .find(|p| body.starts_with(**p))
        .copied()
        .unwrap_or_default();
    let value = parse_digits(&body[prefix.len()..], radix_of(prefix))
        .with_context(|| format!("`{token}` is not a valid Rust integer literal"))?;
    ensure!(
        value == expected,
        "integer literal `{token}` has value {value}, but the site's constant is {expected}",
    );
    Ok(format!("{prefix}{digit}{suffix}"))
}

/// Rewrites a plain decimal float literal to `digits` (`"0.0"` or `"1.0"`),
/// keeping any `f32`/`f64` suffix so the mutant still compiles with the original
/// type.
///
/// The token is validated first: it must be the *plain decimal* form the site was
/// discovered for (see the module header — exponent, point-less, and
/// trailing-dot spellings are never emitted as sites) **and** its value must be
/// `expected`. Anything else is a token our scanner never emitted, so it errors
/// rather than re-spelling a literal the author wrote differently.
fn mutate_float_literal(token: &str, expected: u8, digits: &str) -> Result<String> {
    let (body, suffix) = split_float_suffix(token);
    let value = plain_decimal_float_value(body)
        .with_context(|| format!("`{token}` is not a plain decimal `0`/`1` float literal"))?;
    ensure!(
        value == expected,
        "float literal `{token}` has value {value}, but the site's constant is {expected}",
    );
    Ok(format!("{digits}{suffix}"))
}

/// The value of a **plain decimal** float literal body: `Some(0)`, `Some(1)`, or
/// `None` when the body is not of that form or names any other value.
///
/// `body` is the literal with its `f32`/`f64` suffix already removed; `_`
/// separators are ignored. The accepted form is `digits "." digits`, which
/// excludes exponent (`1e0`), point-less (`0f64`) and trailing-dot (`0.`)
/// spellings.
///
/// This is the **single definition** of the float precondition: [`crate::scanner`]
/// calls it to decide whether to emit a site at all, so a literal that would be
/// rejected here is never discovered in the first place (precondition-gated
/// emission).
pub(crate) fn plain_decimal_float_value(body: &str) -> Option<u8> {
    let cleaned: String = body.chars().filter(|c| *c != '_').collect();
    let (integer, fraction) = cleaned.split_once('.')?;
    if integer.is_empty() || fraction.is_empty() {
        return None;
    }
    if !integer.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c == '0') {
        return None;
    }
    match integer.trim_start_matches('0') {
        "" => Some(0),
        "1" => Some(1),
        _ => None,
    }
}

/// Splits a float literal into `(body, suffix)`, where `suffix` is `f32`, `f64`,
/// or the empty string.
fn split_float_suffix(token: &str) -> (&str, &str) {
    FLOAT_SUFFIXES
        .iter()
        .find_map(|suffix| {
            token
                .strip_suffix(suffix)
                .filter(|body| !body.is_empty())
                .map(|body| (body, *suffix))
        })
        .unwrap_or((token, ""))
}

/// The numeric base a radix prefix denotes; decimal when there is no prefix.
fn radix_of(prefix: &str) -> u32 {
    match prefix {
        "0x" | "0X" => 16,
        "0o" | "0O" => 8,
        "0b" | "0B" => 2,
        _ => 10,
    }
}

/// Parses the digit body of an integer literal (Rust's `_` separators allowed) in
/// `radix`.
///
/// # Errors
///
/// Returns an error if the body is empty, holds a character that is not a digit
/// of `radix`, or names a value too large for `u128`.
fn parse_digits(digits: &str, radix: u32) -> Result<u128> {
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    ensure!(!cleaned.is_empty(), "it carries no digits");
    ensure!(
        cleaned.chars().all(|c| c.is_digit(radix)),
        "`{cleaned}` is not a base-{radix} digit sequence",
    );
    u128::from_str_radix(&cleaned, radix).context("its value does not fit in u128")
}

/// Splits an integer literal into `(body, suffix)`, where `suffix` is a Rust
/// integer suffix or the empty string.
fn split_suffix(token: &str) -> (&str, &str) {
    INT_SUFFIXES
        .iter()
        .find_map(|suffix| {
            token
                .strip_suffix(suffix)
                .filter(|body| !body.is_empty())
                .map(|body| (body, *suffix))
        })
        .unwrap_or((token, ""))
}

#[cfg(test)]
mod tests {
    use super::{edits, replacement};
    use crate::site::{Edit, Operator, Site};

    /// Convenience: the mutated token for a fixed-mapping operator.
    fn map(operator: Operator, token: &str) -> String {
        replacement(operator, token).expect("mapping should succeed")
    }

    #[test]
    fn arithmetic_parity_mappings() {
        // Byte-identical to mutate4go — S4 must not have disturbed these.
        assert_eq!(map(Operator::Add, "+"), "-");
        assert_eq!(map(Operator::Sub, "-"), "+");
        assert_eq!(map(Operator::Mul, "*"), "/");
    }

    #[test]
    fn arithmetic_idiomatic_completion_mappings() {
        // Deliberately asymmetric: `* → /` (parity) and `/ → *` (idiomatic) both
        // exist; `% → *` is one-way.
        assert_eq!(map(Operator::Div, "/"), "*");
        assert_eq!(map(Operator::Rem, "%"), "*");
    }

    #[test]
    fn compound_assignment_mappings() {
        // `+=`/`-=` are bidirectional; `*=`/`/=` swap; `%= → *=` is one-way.
        assert_eq!(map(Operator::AddAssign, "+="), "-=");
        assert_eq!(map(Operator::SubAssign, "-="), "+=");
        assert_eq!(map(Operator::MulAssign, "*="), "/=");
        assert_eq!(map(Operator::DivAssign, "/="), "*=");
        assert_eq!(map(Operator::RemAssign, "%="), "*=");
    }

    #[test]
    fn comparison_mappings_swap_strictness() {
        assert_eq!(map(Operator::Greater, ">"), ">=");
        assert_eq!(map(Operator::GreaterEqual, ">="), ">");
        assert_eq!(map(Operator::Less, "<"), "<=");
        assert_eq!(map(Operator::LessEqual, "<="), "<");
    }

    #[test]
    fn equality_mappings() {
        assert_eq!(map(Operator::Equal, "=="), "!=");
        assert_eq!(map(Operator::NotEqual, "!="), "==");
    }

    #[test]
    fn boolean_literal_mappings() {
        assert_eq!(map(Operator::True, "true"), "false");
        assert_eq!(map(Operator::False, "false"), "true");
    }

    #[test]
    fn logical_mappings() {
        assert_eq!(map(Operator::And, "&&"), "||");
        assert_eq!(map(Operator::Or, "||"), "&&");
    }

    #[test]
    fn constant_mappings_swap_zero_and_one() {
        assert_eq!(map(Operator::Zero, "0"), "1");
        assert_eq!(map(Operator::One, "1"), "0");
    }

    #[test]
    fn bitwise_and_shift_mappings() {
        // `&`/`|` are bidirectional; `^ → &` is one-way, exactly as the
        // arithmetic table has one-way entries. Shifts swap direction.
        assert_eq!(map(Operator::BitAnd, "&"), "|");
        assert_eq!(map(Operator::BitOr, "|"), "&");
        assert_eq!(map(Operator::BitXor, "^"), "&");
        assert_eq!(map(Operator::Shl, "<<"), ">>");
        assert_eq!(map(Operator::Shr, ">>"), "<<");
    }

    #[test]
    fn predicate_method_mappings_swap_the_method_name_only() {
        // The site's span is the method identifier, so the replacement is the
        // bare name — never `.is_none()` and never the receiver.
        assert_eq!(map(Operator::IsSome, "is_some"), "is_none");
        assert_eq!(map(Operator::IsNone, "is_none"), "is_some");
        assert_eq!(map(Operator::IsOk, "is_ok"), "is_err");
        assert_eq!(map(Operator::IsErr, "is_err"), "is_ok");
    }

    #[test]
    fn float_constant_mappings_swap_zero_and_one() {
        assert_eq!(map(Operator::FloatZero, "0.0"), "1.0");
        assert_eq!(map(Operator::FloatOne, "1.0"), "0.0");
    }

    #[test]
    fn float_mapping_preserves_the_suffix() {
        // Dropping `f32` would change the mutant's type and could break
        // inference — the suffix must survive verbatim.
        assert_eq!(map(Operator::FloatOne, "1.0f32"), "0.0f32");
        assert_eq!(map(Operator::FloatZero, "0.0f64"), "1.0f64");
        // A separator before the suffix is dropped, exactly as on the integer
        // path; the result is still valid Rust of the same type.
        assert_eq!(map(Operator::FloatZero, "0.0_f32"), "1.0f32");
    }

    #[test]
    fn float_mapping_canonicalizes_redundant_digits() {
        // Extra zeros are the same plain decimal value, so they are in scope and
        // collapse to the canonical spelling.
        assert_eq!(map(Operator::FloatZero, "0.000"), "1.0");
        assert_eq!(map(Operator::FloatOne, "1.00f64"), "0.0f64");
        assert_eq!(map(Operator::FloatZero, "00.0"), "1.0");
    }

    #[test]
    fn float_mapping_rejects_a_form_the_scanner_never_emits() {
        // Exponent, point-less and trailing-dot spellings are never discovered
        // as sites (precondition-gated emission), so reaching the mapping with
        // one is an error rather than a silent re-spelling.
        assert!(replacement(Operator::FloatOne, "1e0").is_err());
        assert!(replacement(Operator::FloatZero, "0.0e0").is_err());
        assert!(replacement(Operator::FloatOne, "1f32").is_err());
        assert!(replacement(Operator::FloatZero, "0.").is_err());
        assert!(replacement(Operator::FloatZero, "").is_err());
        assert!(replacement(Operator::FloatOne, "garbage").is_err());
    }

    #[test]
    fn float_mapping_rejects_a_value_that_is_not_the_operator_constant() {
        assert!(replacement(Operator::FloatOne, "0.0").is_err());
        assert!(replacement(Operator::FloatZero, "1.0").is_err());
        assert!(replacement(Operator::FloatOne, "2.0").is_err());
        assert!(replacement(Operator::FloatZero, "0.5").is_err());
        assert!(replacement(Operator::FloatOne, "1.5f32").is_err());
    }

    /// The float precondition is defined **once** and is what the scanner gates
    /// discovery on, so the two can never disagree about which spellings are in
    /// scope.
    #[test]
    fn the_float_precondition_accepts_only_plain_decimal_zero_and_one() {
        use super::plain_decimal_float_value;

        assert_eq!(plain_decimal_float_value("0.0"), Some(0));
        assert_eq!(plain_decimal_float_value("1.0"), Some(1));
        assert_eq!(plain_decimal_float_value("1.000"), Some(1));
        assert_eq!(plain_decimal_float_value("0_0.0"), Some(0));

        assert_eq!(plain_decimal_float_value("1e0"), None, "exponent form");
        assert_eq!(plain_decimal_float_value("0.0e0"), None, "exponent form");
        assert_eq!(plain_decimal_float_value("0."), None, "trailing dot");
        assert_eq!(plain_decimal_float_value(".0"), None, "leading dot");
        assert_eq!(plain_decimal_float_value("1"), None, "no decimal point");
        assert_eq!(plain_decimal_float_value("2.0"), None, "other value");
        assert_eq!(plain_decimal_float_value("1.5"), None, "other value");
        assert_eq!(plain_decimal_float_value("10.0"), None, "other value");
    }

    #[test]
    fn constant_mapping_preserves_integer_suffix() {
        // A naive splice of "0" over `1u8` would drop the suffix and break type
        // inference — the suffix must survive the mutation.
        assert_eq!(map(Operator::One, "1u8"), "0u8");
        assert_eq!(map(Operator::Zero, "0usize"), "1usize");
        assert_eq!(map(Operator::One, "1i128"), "0i128");
        assert_eq!(map(Operator::Zero, "0isize"), "1isize");
        // A separator before the suffix is dropped, which is still valid Rust.
        assert_eq!(map(Operator::One, "1_u8"), "0u8");
    }

    #[test]
    fn constant_mapping_preserves_radix_prefix() {
        assert_eq!(map(Operator::One, "0x1"), "0x0");
        assert_eq!(map(Operator::Zero, "0x0"), "0x1");
        assert_eq!(map(Operator::One, "0o1"), "0o0");
        assert_eq!(map(Operator::One, "0b1"), "0b0");
        assert_eq!(map(Operator::Zero, "0B0"), "0B1");
        // Grouped binary digits collapse to a single digit of the same radix.
        assert_eq!(map(Operator::One, "0b0000_0001"), "0b0");
    }

    #[test]
    fn constant_mapping_preserves_radix_and_suffix_together() {
        assert_eq!(map(Operator::One, "0x1u8"), "0x0u8");
        assert_eq!(map(Operator::Zero, "0b0usize"), "0b1usize");
    }

    #[test]
    fn constant_mapping_rejects_a_malformed_literal() {
        // Cannot come from our scanner; the mapping errors rather than panicking —
        // and must never silently normalize garbage into a valid literal.
        assert!(replacement(Operator::One, "0x").is_err());
        assert!(replacement(Operator::Zero, "").is_err());
        assert!(replacement(Operator::One, "garbage").is_err());
        assert!(replacement(Operator::One, "0xg").is_err());
        assert!(replacement(Operator::One, "1bogus").is_err());
        assert!(replacement(Operator::Zero, "0b2").is_err());
        // `0f64` is a float literal wearing an integer's clothes; the scanner
        // filters it out, and the mapping refuses it too.
        assert!(replacement(Operator::Zero, "0f64").is_err());
    }

    /// The suffix classifier is the single rule keeping a float-suffixed
    /// `syn::LitInt` out of the integer constant class.
    #[test]
    fn only_integer_suffixes_are_integer_suffixes() {
        use super::is_integer_suffix;

        assert!(is_integer_suffix(""));
        assert!(is_integer_suffix("u8"));
        assert!(is_integer_suffix("usize"));
        assert!(is_integer_suffix("i128"));
        assert!(!is_integer_suffix("f32"));
        assert!(!is_integer_suffix("f64"));
    }

    #[test]
    fn constant_mapping_rejects_a_value_that_is_not_the_operator_constant() {
        // The token must actually be the constant the site was discovered for.
        assert!(replacement(Operator::One, "0").is_err());
        assert!(replacement(Operator::Zero, "1").is_err());
        assert!(replacement(Operator::One, "2").is_err());
        assert!(replacement(Operator::Zero, "0x10u8").is_err());
    }

    #[test]
    fn structural_mappings_are_token_independent() {
        // Each is one span the scanner chose so a single replacement suffices;
        // the token is whatever the author wrote there and is not consulted.
        assert_eq!(map(Operator::SomeCall, "Some(x)"), "None");
        assert_eq!(map(Operator::OkCall, "Ok"), "Err");
        assert_eq!(map(Operator::Unwrap, "unwrap"), "unwrap_or_default");
        assert_eq!(
            map(Operator::Expect, "expect(\"boom\")"),
            "unwrap_or_default()"
        );
        assert_eq!(map(Operator::Try, "?"), ".unwrap()");
        assert_eq!(map(Operator::ArmGuard, "if x > 2"), "");
    }

    /// The arm body swap is **not** on the token-for-token fast path: its
    /// replacement text is source at another location in the file, so
    /// [`replacement`] cannot produce it for any token and says so rather than
    /// inventing one. [`edits`] is its only route.
    #[test]
    fn the_arm_body_swap_has_no_token_for_token_replacement() {
        assert!(replacement(Operator::ArmBodySwap, "y").is_err());
        assert!(replacement(Operator::ArmBodySwap, "").is_err());
    }

    /// The two edits of an arm-body swap are the two spans trading source text —
    /// each carries the *other* span's bytes, which is precisely what
    /// [`replacement`] cannot compute.
    #[test]
    fn the_swap_path_builds_two_edits_that_trade_source_text() {
        let source = "fn f(x: i32) -> i32 { match x { 2 => 7, _ => 9 } }\n";
        let first = source.find('7').expect("first body");
        let second = source.find('9').expect("second body");
        let site = Site::swapping(
            Operator::ArmBodySwap,
            first..first + 1,
            second..second + 1,
            1,
            1,
            None,
        );

        assert_eq!(
            edits(source, &site).expect("valid spans"),
            vec![
                Edit::new(first..first + 1, "9".to_owned()),
                Edit::new(second..second + 1, "7".to_owned()),
            ],
        );
    }

    /// Every other operator stays on the **single-token fast path**: one edit
    /// over the site's own span, carrying exactly what [`replacement`] returns.
    #[test]
    fn the_fast_path_builds_exactly_one_edit_from_the_mapping() {
        let source = "fn f(a: i32, b: i32) -> i32 { a + b }\n";
        let plus = source.find('+').expect("the operator token");
        let site = Site::new(Operator::Add, plus..plus + 1, 1, None);

        assert_eq!(
            edits(source, &site).expect("valid span"),
            vec![Edit::new(plus..plus + 1, "-".to_owned())],
        );
    }

    /// A span outside the source is an error naming the line, never a panic.
    #[test]
    fn edits_reject_a_span_outside_the_source() {
        let source = "fn f() -> i32 { 1 }\n";
        let site = Site::new(Operator::One, 900..901, 1, None);

        let err = edits(source, &site).expect_err("span is outside the source");
        assert!(
            err.to_string().contains("line 1"),
            "the error must locate the site: {err}",
        );
    }

    /// Mutating an in-scope site always changes the token — a mapping that
    /// returned the original would produce an equivalent (never-killable) mutant.
    ///
    /// Driven by [`Operator::ALL`] and [`Operator::canonical_token`], so a new
    /// operator is covered **by construction**: `ALL` is generated from the same
    /// variant list as the enum, so a variant cannot exist outside it. Structural
    /// operators have no canonical token; the equivalent property for them is the
    /// scanner's composition test, which asserts the exact mutant source.
    #[test]
    fn every_mapping_changes_the_token() {
        for operator in Operator::ALL {
            let Some(token) = operator.canonical_token() else {
                continue;
            };
            assert_ne!(map(operator, token), token, "{operator:?} is a no-op");
        }
    }

    /// The exact description a **token-less** structural operator must render,
    /// or `None` for an operator that has a canonical token (whose description is
    /// *derived* from the mapping below instead of pinned).
    ///
    /// A structural operator's replacement is not a function of any token, so
    /// there is no string to derive and the honest form is an exact table. The
    /// split is checked against [`Operator::canonical_token`] over the whole
    /// [`Operator::ALL`] roster, so a new token-less operator with no row here
    /// fails loudly rather than going unchecked — the `declare_operators!`
    /// standard applied to a test table.
    fn structural_description(operator: Operator) -> Option<&'static str> {
        match operator {
            Operator::SomeCall => Some("`Some(_)` → `None`"),
            Operator::OkCall => Some("`Ok(_)` → `Err(_)`"),
            Operator::Expect => Some("`expect(_)` → `unwrap_or_default()`"),
            Operator::ArmGuard => Some("`if <guard>` → dropped"),
            Operator::ArmBodySwap => Some("`match` arm bodies swapped"),
            _ => None,
        }
    }

    /// The record's rendering and the mapping cannot drift: for every operator
    /// with a canonical token, the description is exactly that token and what it
    /// maps to; for the structural operators, which have no token, it is the
    /// exact pinned string above — a containment check would accept an arbitrary
    /// left-hand side, so `Ok(_) → Err(_)` could silently become `Err(_) →
    /// Err(_)`, a self-contradictory no-op in the survivor listing.
    ///
    /// The pinned string is additionally checked to mention what is actually
    /// spliced, so the table cannot drift away from [`replacement`] either.
    ///
    /// The `_ => None` wildcard in [`structural_description`] is **not** the
    /// hand-kept list design.md forbids: the enforcement lives in **the pair**.
    /// [`Operator::canonical_token`] is exhaustive by construction — it has no
    /// wildcard — so a new token-less operator necessarily yields `(None, None)`
    /// here and hits the `panic!`. `canonical_token`'s exhaustiveness is
    /// load-bearing; `structural_description` is a lookup, not a roster.
    #[test]
    fn description_matches_the_mapping() {
        for operator in Operator::ALL {
            match (operator.canonical_token(), structural_description(operator)) {
                (Some(token), None) => assert_eq!(
                    operator.description(),
                    format!("`{token}` → `{}`", map(operator, token)),
                    "{operator:?}",
                ),
                (None, Some(pinned)) => {
                    assert_eq!(operator.description(), pinned, "{operator:?}");
                    // A token-less operator that is still on the single-token
                    // fast path must say what it splices. The one that is not —
                    // the arm body swap, whose replacement is source elsewhere in
                    // the file — has no such string by construction; its
                    // rendering is anchored by the scanner's composition test.
                    match replacement(operator, "") {
                        Ok(spliced) if spliced.is_empty() => assert!(
                            pinned.contains("dropped"),
                            "{operator:?} splices nothing but does not say so",
                        ),
                        Ok(spliced) => assert!(
                            pinned.contains(&spliced),
                            "{operator:?} describes a replacement it does not splice",
                        ),
                        Err(_) => assert!(
                            operator == Operator::ArmBodySwap,
                            "{operator:?} has no mapping and no exemption",
                        ),
                    }
                }
                (Some(_), Some(_)) | (None, None) => panic!(
                    "{operator:?} must have exactly one of a canonical token or a pinned \
                     structural description",
                ),
            }
        }
    }
}
