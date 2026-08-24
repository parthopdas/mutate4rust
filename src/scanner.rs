//! `syn`/`proc-macro2` mutation-site scanner (Core/domain).
//!
//! Parses a Rust source string with `syn` and walks it with the **immutable**
//! [`syn::visit::Visit`] visitor (never `visit_mut` — we do not `syn`-reprint the
//! file) to collect [`Site`]s. Byte spans come straight from `proc-macro2`
//! fallback offsets so mutations can later be applied as byte-span splices over
//! the original bytes (design O1/C6, risk R2).
//!
//! This module is **Core/domain**: fs-/process-/argv-/clap-free, and it never
//! `use`s the runner, coverage, or CLI layers. Reading a file from disk belongs
//! in a later infrastructure adapter, not here — the scanner takes source text.

use anyhow::{Context, Result};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::site::{Operator, Site};

/// Parses `source` as a Rust file and returns every T4-scope mutation site, in
/// deterministic source order (depth-first, as visited).
///
/// # Errors
///
/// Returns an error if `source` is not valid Rust — discovery fails fast with a
/// clear message rather than silently skipping the file (assumption A1).
pub fn scan_source(source: &str) -> Result<Vec<Site>> {
    let file =
        syn::parse_file(source).context("failed to parse Rust source for mutation scanning")?;
    let mut collector = SiteCollector::new();
    collector.visit_file(&file);
    Ok(collector.sites)
}

/// A lexical scope pushed while walking. Functions contribute to a site's
/// `function_id`; modules and impl/trait types only *qualify* a nested function's
/// path, they do not by themselves make a site "inside a function".
///
/// `pub(crate)` so the manifest seam ([`crate::manifest`]) can reuse the exact
/// same scope-stack scheme, keeping its per-function keys identical to the
/// scanner's [`Site::function_id`](crate::site::Site::function_id).
pub(crate) enum Frame {
    /// A function/method body — sites directly inside belong to this function.
    Function(String),
    /// A non-function qualifier (module, impl self-type, or trait) that prefixes a
    /// nested function's path.
    Qualifier(String),
}

impl Frame {
    pub(crate) fn segment(&self) -> &str {
        match self {
            Frame::Function(name) | Frame::Qualifier(name) => name,
        }
    }
}

/// Joins the enclosing scope segments into a `::`-separated path (e.g.
/// `S::method::nested`). Shared with [`crate::manifest`] so function identity is
/// derived one way only.
pub(crate) fn scope_path(scope: &[Frame]) -> String {
    scope
        .iter()
        .map(Frame::segment)
        .collect::<Vec<_>>()
        .join("::")
}

/// Immutable-visitor state: the collected sites plus the enclosing scope stack.
struct SiteCollector {
    sites: Vec<Site>,
    scope: Vec<Frame>,
}

impl SiteCollector {
    fn new() -> Self {
        Self {
            sites: Vec::new(),
            scope: Vec::new(),
        }
    }

    /// The enclosing function's fully-qualified path, or `None` when the nearest
    /// enclosing scope is not a function (module/impl/trait level). Closures add
    /// no frame, so a site inside a closure is attributed to the enclosing
    /// function.
    fn function_id(&self) -> Option<String> {
        match self.scope.last() {
            Some(Frame::Function(_)) => Some(scope_path(&self.scope)),
            _ => None,
        }
    }

    fn push_site(&mut self, operator: Operator, span: Span) {
        let byte_span = span.byte_range();
        let line = span.start().line;
        let function_id = self.function_id();
        self.sites
            .push(Site::new(operator, byte_span, line, function_id));
    }
}

impl<'ast> Visit<'ast> for SiteCollector {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        visit::visit_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        visit::visit_impl_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        visit::visit_trait_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.scope
            .push(Frame::Qualifier(self_ty_name(&node.self_ty)));
        visit::visit_item_impl(self, node);
        self.scope.pop();
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.scope.push(Frame::Qualifier(node.ident.to_string()));
        visit::visit_item_trait(self, node);
        self.scope.pop();
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.scope.push(Frame::Qualifier(node.ident.to_string()));
        visit::visit_item_mod(self, node);
        self.scope.pop();
    }

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if let Some(operator) = binary_operator(&node.op) {
            self.push_site(operator, node.op.span());
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_lit_bool(&mut self, node: &'ast syn::LitBool) {
        let operator = if node.value {
            Operator::True
        } else {
            Operator::False
        };
        self.push_site(operator, node.span);
    }

    fn visit_lit_int(&mut self, node: &'ast syn::LitInt) {
        if let Some(operator) = constant_operator(node) {
            self.push_site(operator, node.span());
        }
    }
}

/// Maps a binary operator onto its T4-scope [`Operator`], or `None` for operators
/// deferred to later slices (`/`, `%`, bitwise, shifts, compound assignment).
fn binary_operator(op: &syn::BinOp) -> Option<Operator> {
    match op {
        syn::BinOp::Add(_) => Some(Operator::Add),
        syn::BinOp::Sub(_) => Some(Operator::Sub),
        syn::BinOp::Mul(_) => Some(Operator::Mul),
        syn::BinOp::Gt(_) => Some(Operator::Greater),
        syn::BinOp::Ge(_) => Some(Operator::GreaterEqual),
        syn::BinOp::Lt(_) => Some(Operator::Less),
        syn::BinOp::Le(_) => Some(Operator::LessEqual),
        syn::BinOp::Eq(_) => Some(Operator::Equal),
        syn::BinOp::Ne(_) => Some(Operator::NotEqual),
        syn::BinOp::And(_) => Some(Operator::And),
        syn::BinOp::Or(_) => Some(Operator::Or),
        _ => None,
    }
}

/// Maps an integer literal whose base-10 value is `0` or `1` onto the matching
/// constant [`Operator`]. `base10_digits` is radix-normalized by `syn`, so this
/// also matches e.g. `0x1` (value `1`); other values are out of scope.
fn constant_operator(lit: &syn::LitInt) -> Option<Operator> {
    match lit.base10_digits() {
        "0" => Some(Operator::Zero),
        "1" => Some(Operator::One),
        _ => None,
    }
}

/// Best-effort simple name of an `impl` self-type for path qualification (e.g.
/// `Foo` from `impl Foo`). Non-path types fall back to `_`.
///
/// `pub(crate)` so [`crate::manifest`] resolves impl qualifiers identically.
pub(crate) fn self_ty_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map_or_else(|| "_".to_owned(), |seg| seg.ident.to_string()),
        _ => "_".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::scan_source;
    use crate::site::{Operator, Site, SiteKind};

    /// Returns the exact source text covered by a site's byte span.
    fn span_text<'a>(source: &'a str, site: &Site) -> &'a str {
        &source[site.byte_span.start..site.byte_span.end]
    }

    /// Convenience: the ordered `(operator, span-text)` pairs for a source.
    fn scan(source: &str) -> Vec<Site> {
        scan_source(source).expect("fixture should parse")
    }

    #[test]
    fn arithmetic_parity_operators_only() {
        // `/` is an idiomatic completion (S4), so it must NOT be discovered here.
        let source = concat!(
            "fn add(a: i32, b: i32) -> i32 { a + b }\n",
            "fn sub(a: i32, b: i32) -> i32 { a - b }\n",
            "fn mul(a: i32, b: i32) -> i32 { a * b }\n",
            "fn div(a: i32, b: i32) -> i32 { a / b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 3, "only +, -, * are in T4 scope");
        assert!(sites.iter().all(|s| s.kind == SiteKind::Arithmetic));
        assert_eq!(sites[0].operator, Operator::Add);
        assert_eq!(span_text(source, &sites[0]), "+");
        assert_eq!(sites[1].operator, Operator::Sub);
        assert_eq!(span_text(source, &sites[1]), "-");
        assert_eq!(sites[2].operator, Operator::Mul);
        assert_eq!(span_text(source, &sites[2]), "*");
        assert_eq!(
            sites
                .iter()
                .map(|s| s.function_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("add"), Some("sub"), Some("mul")],
        );
    }

    #[test]
    fn deferred_operators_emit_no_sites() {
        // Every operator deferred to a later slice — `%`, bitwise `& | ^`, shifts
        // `<< >>`, and compound-assignment `+= -= *= /= %=` — must yield NO site.
        // This locks the T4 scope boundary. Operands are non-literal params so no
        // in-scope constant/boolean sites can sneak in.
        let source = concat!(
            "fn rem(a: i32, b: i32) -> i32 { a % b }\n",
            "fn bit_and(a: i32, b: i32) -> i32 { a & b }\n",
            "fn bit_or(a: i32, b: i32) -> i32 { a | b }\n",
            "fn bit_xor(a: i32, b: i32) -> i32 { a ^ b }\n",
            "fn shl(a: i32, b: i32) -> i32 { a << b }\n",
            "fn shr(a: i32, b: i32) -> i32 { a >> b }\n",
            "fn compound(mut a: i32, b: i32) {\n",
            "    a += b;\n",
            "    a -= b;\n",
            "    a *= b;\n",
            "    a /= b;\n",
            "    a %= b;\n",
            "}\n",
        );
        let sites = scan(source);

        assert!(
            sites.is_empty(),
            "no deferred operator is in T4 scope, got {sites:?}",
        );
    }

    #[test]
    fn byte_span_is_a_true_byte_offset_past_multibyte_utf8() {
        // A multibyte UTF-8 string literal precedes the `+`. If `byte_span` were a
        // char/column offset, slicing here would land mid-token; asserting the
        // slice is exactly "+" proves the offsets are real byte offsets — the O1
        // byte-span-splice invariant.
        let source = "fn f(a: i32, b: i32) -> i32 { let _s = \"café ☕ dé\"; a + b }\n";
        let sites = scan(source);

        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].operator, Operator::Add);
        assert_eq!(span_text(source, &sites[0]), "+");
        // The multibyte prefix makes the byte offset strictly exceed the char
        // offset, so this genuinely exercises the byte-vs-char distinction.
        let char_offset = source.chars().take_while(|&c| c != '+').count();
        assert!(
            sites[0].byte_span.start > char_offset,
            "byte offset {} should exceed char offset {char_offset}",
            sites[0].byte_span.start,
        );
    }

    #[test]
    fn comparison_operators_span_multi_char_tokens() {
        let source = concat!(
            "fn gt(a: i32, b: i32) -> bool { a > b }\n",
            "fn ge(a: i32, b: i32) -> bool { a >= b }\n",
            "fn lt(a: i32, b: i32) -> bool { a < b }\n",
            "fn le(a: i32, b: i32) -> bool { a <= b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 4);
        assert!(sites.iter().all(|s| s.kind == SiteKind::Comparison));
        let found: Vec<(Operator, &str)> = sites
            .iter()
            .map(|s| (s.operator, span_text(source, s)))
            .collect();
        assert_eq!(
            found,
            vec![
                (Operator::Greater, ">"),
                (Operator::GreaterEqual, ">="),
                (Operator::Less, "<"),
                (Operator::LessEqual, "<="),
            ],
        );
    }

    #[test]
    fn equality_operators() {
        let source = concat!(
            "fn eq(a: i32, b: i32) -> bool { a == b }\n",
            "fn ne(a: i32, b: i32) -> bool { a != b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|s| s.kind == SiteKind::Equality));
        assert_eq!(sites[0].operator, Operator::Equal);
        assert_eq!(span_text(source, &sites[0]), "==");
        assert_eq!(sites[1].operator, Operator::NotEqual);
        assert_eq!(span_text(source, &sites[1]), "!=");
    }

    #[test]
    fn logical_operators() {
        let source = concat!(
            "fn and(a: bool, b: bool) -> bool { a && b }\n",
            "fn or(a: bool, b: bool) -> bool { a || b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|s| s.kind == SiteKind::Logical));
        assert_eq!(sites[0].operator, Operator::And);
        assert_eq!(span_text(source, &sites[0]), "&&");
        assert_eq!(sites[1].operator, Operator::Or);
        assert_eq!(span_text(source, &sites[1]), "||");
    }

    #[test]
    fn boolean_literals() {
        let source = concat!("fn t() -> bool { true }\n", "fn f() -> bool { false }\n",);
        let sites = scan(source);

        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|s| s.kind == SiteKind::BooleanLiteral));
        assert_eq!(sites[0].operator, Operator::True);
        assert_eq!(span_text(source, &sites[0]), "true");
        assert_eq!(sites[1].operator, Operator::False);
        assert_eq!(span_text(source, &sites[1]), "false");
    }

    #[test]
    fn integer_constants_zero_and_one_only() {
        // `2` is not a `0`/`1` constant, so it must NOT be discovered.
        let source = concat!(
            "fn zero() -> i32 { 0 }\n",
            "fn one() -> i32 { 1 }\n",
            "fn two() -> i32 { 2 }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 2, "only 0 and 1 are constant sites");
        assert!(sites.iter().all(|s| s.kind == SiteKind::Constant));
        assert_eq!(sites[0].operator, Operator::Zero);
        assert_eq!(span_text(source, &sites[0]), "0");
        assert_eq!(sites[1].operator, Operator::One);
        assert_eq!(span_text(source, &sites[1]), "1");
    }

    #[test]
    fn line_numbers_are_one_based() {
        let source = "fn f(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        let sites = scan(source);

        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].line, 2);
        assert_eq!(span_text(source, &sites[0]), "+");
    }

    #[test]
    fn function_id_groups_free_module_and_impl_functions() {
        let source = concat!(
            "fn top(a: i32, b: i32) -> i32 { a + b }\n",
            "mod m {\n",
            "    pub fn inner(a: i32, b: i32) -> i32 { a - b }\n",
            "}\n",
            "struct S;\n",
            "impl S {\n",
            "    fn method(a: i32, b: i32) -> i32 { a * b }\n",
            "}\n",
        );
        let sites = scan(source);

        let grouped: Vec<(Operator, Option<&str>)> = sites
            .iter()
            .map(|s| (s.operator, s.function_id.as_deref()))
            .collect();
        assert_eq!(
            grouped,
            vec![
                (Operator::Add, Some("top")),
                (Operator::Sub, Some("m::inner")),
                (Operator::Mul, Some("S::method")),
            ],
        );
    }

    #[test]
    fn nested_fn_gets_own_id_and_closure_belongs_to_enclosing_fn() {
        // Edge case: a nested `fn` is its own scope (`outer::nested`); a closure
        // adds no scope, so its operator is attributed to `outer`.
        let source = concat!(
            "fn outer(a: i32, b: i32) -> i32 {\n",
            "    fn nested(x: i32, y: i32) -> i32 { x + y }\n",
            "    let sub = |p: i32, q: i32| p - q;\n",
            "    nested(a, b) + sub(a, b)\n",
            "}\n",
        );
        let sites = scan(source);

        let grouped: Vec<(Operator, Option<&str>)> = sites
            .iter()
            .map(|s| (s.operator, s.function_id.as_deref()))
            .collect();
        assert_eq!(
            grouped,
            vec![
                (Operator::Add, Some("outer::nested")),
                (Operator::Sub, Some("outer")),
                (Operator::Add, Some("outer")),
            ],
        );
    }

    #[test]
    fn sites_outside_any_function_have_no_function_id() {
        // A module-level const and an associated const are not inside a function
        // body, so `function_id` is `None` (documented representation choice).
        let source = concat!(
            "const X: i32 = 0;\n",
            "struct S;\n",
            "impl S {\n",
            "    const C: i32 = 1;\n",
            "}\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|s| s.function_id.is_none()));
        assert_eq!(sites[0].operator, Operator::Zero);
        assert_eq!(sites[1].operator, Operator::One);
    }

    #[test]
    fn unparseable_source_is_an_error() {
        assert!(scan_source("fn broken(").is_err());
    }
}
