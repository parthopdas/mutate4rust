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

use anyhow::{Context, Result, ensure};

use crate::site::Operator;

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
    use super::replacement;
    use crate::site::Operator;

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

    /// Mutating an in-scope site always changes the token — a mapping that
    /// returned the original would produce an equivalent (never-killable) mutant.
    ///
    /// Driven by [`Operator::ALL`] and [`Operator::canonical_token`], so a new
    /// operator is covered **by construction**: `ALL` is generated from the same
    /// variant list as the enum, so a variant cannot exist outside it.
    #[test]
    fn every_mapping_changes_the_token() {
        for operator in Operator::ALL {
            let token = operator.canonical_token();
            assert_ne!(map(operator, token), token, "{operator:?} is a no-op");
        }
    }
}
