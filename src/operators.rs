//! Operator mappings: a discovered site's original token → its mutated token
//! (Core/domain).
//!
//! This module is **pure** (design.md → inward dependency flow): it takes token
//! text and returns token text, with no fs/process/argv/clap dependency and no
//! knowledge of the runner, coverage, or CLI layers. The caller slices the token
//! out of the original source using [`crate::site::Site::byte_span`] and splices
//! the returned replacement back with [`crate::apply::splice`].
//!
//! # Scope — the S3 universal + arithmetic-**parity** set
//!
//! | Class | Mapping |
//! |-------|---------|
//! | Arithmetic | `+ → -`; `- → +`; `* → /` |
//! | Comparison | `> → >=`; `>= → >`; `< → <=`; `<= → <` |
//! | Equality | `== → !=`; `!= → ==` |
//! | Boolean literal | `true → false`; `false → true` |
//! | Logical | `&& → \|\|`; `\|\| → &&` |
//! | Constant | `0 → 1`; `1 → 0` |
//!
//! `/ → *`, `% → *`, and the compound-assignment operators are the S4 idiomatic
//! completions and are deliberately absent here — the scanner does not emit those
//! sites yet either.
//!
//! # Integer literals keep their radix and suffix
//!
//! A constant site's byte span covers the **whole literal token**, so a naive
//! splice of `"0"` over `1u8` would drop the suffix and break type inference, and
//! over `0x1` would silently change the radix in the diff. [`replacement`]
//! therefore rewrites only the digits, preserving any `0x`/`0o`/`0b` radix prefix
//! and any integer suffix: `1u8 → 0u8`, `0x1 → 0x0`, `0b0 → 0b1`.
//!
//! **Floats are not constant sites.** The scanner matches `syn::LitInt` only, so
//! `0.0`/`1.0` never reach this module and no float mapping exists.

use anyhow::{Context, Result, ensure};

use crate::site::Operator;

/// Every Rust integer-literal suffix, longest first so a strip is unambiguous.
const INT_SUFFIXES: [&str; 12] = [
    "usize", "isize", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
];

/// Every Rust integer-literal radix prefix.
const RADIX_PREFIXES: [&str; 6] = ["0x", "0X", "0o", "0O", "0b", "0B"];

/// The mutated token text for `operator`, given the site's original token text.
///
/// `token` is only consulted for [`Operator::Zero`]/[`Operator::One`], where the
/// literal's radix prefix and suffix must be preserved; every other mapping is a
/// fixed token-for-token swap.
///
/// # Errors
///
/// Returns an error — never panics — if a constant site's token is not a
/// well-formed Rust integer literal, or if its value is not the constant the
/// operator was discovered for (neither could have come from our own scanner).
pub(crate) fn replacement(operator: Operator, token: &str) -> Result<String> {
    let mapped = match operator {
        Operator::Add => "-",
        Operator::Sub => "+",
        Operator::Mul => "/",
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
        Operator::Zero => return mutate_int_literal(token, 0, '1'),
        Operator::One => return mutate_int_literal(token, 1, '0'),
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
        // Parity subset only — `/` and `%` are S4 and have no Operator variant.
        assert_eq!(map(Operator::Add, "+"), "-");
        assert_eq!(map(Operator::Sub, "-"), "+");
        assert_eq!(map(Operator::Mul, "*"), "/");
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
    #[test]
    fn every_mapping_changes_the_token() {
        for (operator, token) in [
            (Operator::Add, "+"),
            (Operator::Sub, "-"),
            (Operator::Mul, "*"),
            (Operator::Greater, ">"),
            (Operator::GreaterEqual, ">="),
            (Operator::Less, "<"),
            (Operator::LessEqual, "<="),
            (Operator::Equal, "=="),
            (Operator::NotEqual, "!="),
            (Operator::And, "&&"),
            (Operator::Or, "||"),
            (Operator::True, "true"),
            (Operator::False, "false"),
            (Operator::Zero, "0"),
            (Operator::One, "1"),
        ] {
            assert_ne!(map(operator, token), token, "{operator:?} is a no-op");
        }
    }
}
