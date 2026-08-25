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
//!
//! # `#[cfg(test)]` items are not discovered (deliberate divergence)
//!
//! Items carrying a literal `#[cfg(test)]` attribute — most often the file's own
//! `mod tests` — are skipped outright. Test-module lines are **always covered by
//! construction**, so covered-only gating (S5) would preferentially mutate test
//! code while skipping genuinely uncovered production code; and a mutated
//! assertion constant is killed by its own test, costing a full compile and
//! carrying no signal about test quality. Upstream mutate4go never faced this —
//! Go tests live in a separate `_test.go` file that the one-file-at-a-time model
//! never selects — so this is a **knowing divergence from upstream**, not a port.
//!
//! Only the literal form is recognised (`#[cfg(test)]`); composite predicates such
//! as `#[cfg(any(test, feature = "x"))]` are not treated as test-only.
//!
//! The gate is applied at six points — [`Visit::visit_item`],
//! [`Visit::visit_trait_item`], [`Visit::visit_impl_item`],
//! [`Visit::visit_foreign_item`] (T13a) and [`Visit::visit_stmt`],
//! [`Visit::visit_arm`] (T13b) — each reading its node's attributes through a
//! single `match` over every variant of that enum. The first four are the item
//! enums `syn` 3.0.4 declares in `item.rs`, so an item in a file — free,
//! associated, trait or `extern` block — is reached through one of them, and
//! there is no per-visitor list of gated node types to keep by hand (T13a; a
//! hand-kept list is exactly what `declare_operators!` was introduced to
//! abolish).
//!
//! This claims coverage of *items*, statements and `match` arms, not of every
//! position `syn` 3.0.4 attaches attributes to. The named remaining hole is the
//! **expression statement**: in `#[cfg(test)] foo();` the attribute is parsed
//! onto the `syn::Expr`, not onto the `syn::Stmt`, so [`stmt_attrs`] cannot see
//! it and the call is still scanned. Closing it means reading attributes off
//! every attribute-bearing `Expr` variant, which is a wider audit than T13b
//! bought. `syn` 3.0.4 also carries `attrs` on `Field`, `Variant`, `FnArg`,
//! `GenericParam`, `WherePredicate`, `Pat` and `Type`, and a `#[cfg(test)]`
//! there is **not** gated today either.

use std::ops::Range;

use anyhow::{Context, Result};
use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::operators;
use crate::site::{Operator, Site};

/// Parses `source` as a Rust file and returns every in-scope mutation site, in
/// deterministic source order (depth-first, as visited).
///
/// # Errors
///
/// Returns an error if `source` is not valid Rust — discovery fails fast with a
/// clear message rather than silently skipping the file (assumption A1).
pub fn scan_source(source: &str) -> Result<Vec<Site>> {
    let file = parse(source)?;
    let mut collector = SiteCollector::new();
    collector.visit_file(&file);
    Ok(collector.sites)
}

/// Confirms `source` is valid Rust, discarding the parse tree.
///
/// The pre-flight validation the mutate run performs on its target *before*
/// paying for a coverage build: an unparseable file is a fast, certain error, and
/// learning that after a ~30 s instrumented build is pure waste. This is a
/// **validation, not a stage** — it reorders nothing (R1: coverage still resolves
/// before any mutation).
///
/// # Errors
///
/// Returns an error if `source` is not valid Rust (assumption A1).
pub(crate) fn validate_source(source: &str) -> Result<()> {
    parse(source).map(drop)
}

/// The one `syn::parse_file` call, so discovery and validation fail identically.
fn parse(source: &str) -> Result<syn::File> {
    syn::parse_file(source).context("failed to parse Rust source for mutation scanning")
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
        self.push_span(operator, span.byte_range(), span.start().line);
    }

    /// Records a site from an **assembled** byte span.
    ///
    /// This exists solely because one T13b site has no single `syn::Span` that
    /// describes it: a `match` arm's guard is `if <expr>`, and `syn` 3.0.4 models
    /// it as [`syn::PatGuard`] — the `if` token and the guard expression are
    /// separate nodes, and neither one alone covers the text the mutation drops.
    /// The span is therefore built from the `if` token's start and the guard
    /// expression's end. Every other site comes from exactly one `syn` span and
    /// goes through [`SiteCollector::push_site`].
    fn push_span(&mut self, operator: Operator, byte_span: Range<usize>, line: usize) {
        let function_id = self.function_id();
        self.sites
            .push(Site::new(operator, byte_span, line, function_id));
    }
}

impl<'ast> Visit<'ast> for SiteCollector {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        if is_cfg_test(item_attrs(node)) {
            return;
        }
        visit::visit_item(self, node);
    }

    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        if is_cfg_test(trait_item_attrs(node)) {
            return;
        }
        visit::visit_trait_item(self, node);
    }

    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        if is_cfg_test(impl_item_attrs(node)) {
            return;
        }
        visit::visit_impl_item(self, node);
    }

    fn visit_foreign_item(&mut self, node: &'ast syn::ForeignItem) {
        if is_cfg_test(foreign_item_attrs(node)) {
            return;
        }
        visit::visit_foreign_item(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if is_cfg_test(stmt_attrs(node)) {
            return;
        }
        visit::visit_stmt(self, node);
    }

    /// **Sites live in expressions, never in types** — so the walk stops here and
    /// does not recurse.
    ///
    /// A literal in type position is a compile-time constant: an array length
    /// (`[u8; 1]`) or a const-generic argument. Mutating one is a near-certain
    /// compile error — a wasted build scored `Killed`, i.e. score inflation with
    /// zero signal (design: "Sites live in expressions, never in types").
    ///
    /// This deliberately reduces site counts and is human-approved. Array
    /// **repeat expressions** (`[0u8; 1]`) are unaffected: they are
    /// [`syn::ExprRepeat`], reached through `visit_expr`, and both the element and
    /// the repeat length stay in scope.
    fn visit_type(&mut self, _node: &'ast syn::Type) {}

    /// A `match` arm. Gated for `#[cfg(test)]` like any other attribute-bearing
    /// node, and the source of the [`Operator::ArmGuard`] site.
    ///
    /// **O3 — deliberate span overlap.** A multi-token guard emits *both* an
    /// `ArmGuard` site covering the whole `if <guard>` *and* the operator sites
    /// nested inside the guard expression, whose spans lie strictly within it.
    /// That is intended, not a bug: mutants are applied **one at a time**, each
    /// spliced over pristine source, so two overlapping sites never collide —
    /// they are simply two independent mutants. Combining or de-duplicating
    /// overlapping spans would require the multi-span edit model, which is T13c.
    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        if is_cfg_test(&node.attrs) {
            return;
        }
        if let syn::Pat::Guard(guard) = &node.pat {
            let start = guard.if_token.span().byte_range().start;
            let end = guard.guard.span().byte_range().end;
            self.push_span(
                Operator::ArmGuard,
                start..end,
                guard.if_token.span().start().line,
            );
        }
        visit::visit_arm(self, node);
    }

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

    /// A zero-argument method call (`x.is_some()`, `x.unwrap()`) is a site whose
    /// span is the **method name identifier only**, so the mutation stays a
    /// single-token replacement.
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if let Some(operator) = method_call_operator(node) {
            self.push_site(operator, node.method.span());
        }
        visit::visit_expr_method_call(self, node);
    }

    /// `Some(x)` and `Ok(x)` constructor calls.
    ///
    /// The two take **different spans**, because each one's mapping needs a
    /// different amount of source replaced: `Some(x) → None` discards the bound
    /// value, so the span is the **whole call expression**; `Ok(x) → Err(x)`
    /// keeps it, so the span is the **`Ok` identifier alone** and the argument
    /// list survives the splice untouched.
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let Some(segment) = unary_constructor_segment(node) {
            match segment.ident.to_string().as_str() {
                "Some" => self.push_site(Operator::SomeCall, node.span()),
                "Ok" => self.push_site(Operator::OkCall, segment.ident.span()),
                _ => {}
            }
        }
        visit::visit_expr_call(self, node);
    }

    /// The `?` operator. The span is the **`?` token only**, which the mapping
    /// replaces with `.unwrap()` — the receiver expression is left untouched.
    fn visit_expr_try(&mut self, node: &'ast syn::ExprTry) {
        self.push_site(Operator::Try, node.question_token.span());
        visit::visit_expr_try(self, node);
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

    fn visit_lit_float(&mut self, node: &'ast syn::LitFloat) {
        if let Some(operator) = float_constant_operator(node) {
            self.push_site(operator, node.span());
        }
    }
}

/// The attributes of any `syn::Item`, matched variant by variant.
///
/// Written as a full match rather than a catch-all so that the set of item kinds
/// the `#[cfg(test)]` gate understands is visible in one place. `syn::Item` is
/// `#[non_exhaustive]`, so a trailing arm is unavoidable — it yields **no
/// attributes**, i.e. "keep scanning", which is the same conservative direction
/// [`is_cfg_test`] takes for composite predicates: a variant we do not recognise
/// may still hold production code, and over-scanning is visible where
/// under-scanning is silent.
fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(node) => &node.attrs,
        syn::Item::Enum(node) => &node.attrs,
        syn::Item::ExternCrate(node) => &node.attrs,
        syn::Item::Fn(node) => &node.attrs,
        syn::Item::ForeignMod(node) => &node.attrs,
        syn::Item::Impl(node) => &node.attrs,
        syn::Item::Macro(node) => &node.attrs,
        syn::Item::Mod(node) => &node.attrs,
        syn::Item::Static(node) => &node.attrs,
        syn::Item::Struct(node) => &node.attrs,
        syn::Item::Trait(node) => &node.attrs,
        syn::Item::TraitAlias(node) => &node.attrs,
        syn::Item::Type(node) => &node.attrs,
        syn::Item::Union(node) => &node.attrs,
        syn::Item::Use(node) => &node.attrs,
        syn::Item::Verbatim(_) => &[],
        _ => &[],
    }
}

/// The attributes of any `syn::TraitItem` — see [`item_attrs`] for the shape.
fn trait_item_attrs(item: &syn::TraitItem) -> &[syn::Attribute] {
    match item {
        syn::TraitItem::Const(node) => &node.attrs,
        syn::TraitItem::Fn(node) => &node.attrs,
        syn::TraitItem::Type(node) => &node.attrs,
        syn::TraitItem::Macro(node) => &node.attrs,
        syn::TraitItem::Verbatim(_) => &[],
        _ => &[],
    }
}

/// The attributes of any `syn::ImplItem` — see [`item_attrs`] for the shape.
fn impl_item_attrs(item: &syn::ImplItem) -> &[syn::Attribute] {
    match item {
        syn::ImplItem::Const(node) => &node.attrs,
        syn::ImplItem::Fn(node) => &node.attrs,
        syn::ImplItem::Type(node) => &node.attrs,
        syn::ImplItem::Macro(node) => &node.attrs,
        syn::ImplItem::Verbatim(_) => &[],
        _ => &[],
    }
}

/// The attributes of any `syn::Stmt`, matched variant by variant.
///
/// Unlike the item enums, `syn::Stmt` is **not** `#[non_exhaustive]` in 3.0.4, so
/// this match is exhaustive by the compiler and needs no wildcard: a `syn` bump
/// that adds a variant is a build error here, which is the enforcement the item
/// enums cannot have.
///
/// Only `Stmt::Local` contributes attributes. The other three yield **no
/// attributes** — "keep scanning" — for reasons that differ, so they are
/// enumerated explicitly rather than collapsed into a wildcard (design: enumerate
/// the known variants anyway, so the audited set is readable in one place):
/// `Stmt::Item`'s attributes are the item's own and are already read by
/// [`item_attrs`] through [`Visit::visit_item`]; `Stmt::Expr` carries its
/// attributes on the `syn::Expr`, not on the statement — the expression-statement
/// hole named in the module docs; and `Stmt::Macro`'s body is an unexpanded token
/// stream the scanner never descends into, so gating it would suppress nothing.
fn stmt_attrs(stmt: &syn::Stmt) -> &[syn::Attribute] {
    match stmt {
        syn::Stmt::Local(node) => &node.attrs,
        syn::Stmt::Item(_) | syn::Stmt::Expr(_, _) | syn::Stmt::Macro(_) => &[],
    }
}

/// The attributes of any `syn::ForeignItem` — see [`item_attrs`] for the shape.
///
/// This gate is **vacuous** as of T13b. An `extern` block's items used to reach
/// the scanner because a declaration's *type* can hold an expression — the array
/// length in `static X: [u8; 1];` was a `LitInt` the visitor walked into — but
/// [`Visit::visit_type`] no longer recurses, so nothing inside a foreign item is
/// a site with or without the attribute. Deleting this gate's effect leaves the
/// suite green.
///
/// It stays by **human approval**: it costs nothing, and it keeps the four item
/// enums covered uniformly rather than leaving one conspicuous gap for a reader
/// to re-derive. Note what it does *not* buy — a future `syn` bump that adds a
/// `ForeignItem` variant will **not** be re-validated by this gate, because there
/// is no site behind it to lose. `a_foreign_item_holds_no_site_with_or_without_
/// the_attribute` pins the emptiness itself, and that test does have teeth: it
/// fails if [`Visit::visit_type`] starts recursing again.
fn foreign_item_attrs(item: &syn::ForeignItem) -> &[syn::Attribute] {
    match item {
        syn::ForeignItem::Fn(node) => &node.attrs,
        syn::ForeignItem::Static(node) => &node.attrs,
        syn::ForeignItem::Type(node) => &node.attrs,
        syn::ForeignItem::Macro(node) => &node.attrs,
        syn::ForeignItem::Verbatim(_) => &[],
        _ => &[],
    }
}

/// Whether `attrs` carries a literal `#[cfg(test)]`.
///
/// Matching is deliberately literal — the whole `cfg` predicate must be exactly
/// `test`. A composite such as `#[cfg(any(test, feature = "x"))]` also compiles
/// outside test builds, so treating it as test-only would silently drop
/// production sites; the conservative answer there is to keep scanning.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) => list.path.is_ident("cfg") && list.tokens.to_string() == "test",
        _ => false,
    })
}

/// Maps a binary operator onto its in-scope [`Operator`], or `None` for operators
/// still out of scope (the assign-forms of the bitwise and shift operators, which
/// the taxonomy does not list).
///
/// A compound assignment (`a += b`) arrives as an [`syn::ExprBinary`] whose
/// `op` is the assign variant, and that op's span covers exactly the two-character
/// operator token — not the whole assignment expression — which is what the
/// byte-span splice requires.
fn binary_operator(op: &syn::BinOp) -> Option<Operator> {
    match op {
        syn::BinOp::Add(_) => Some(Operator::Add),
        syn::BinOp::Sub(_) => Some(Operator::Sub),
        syn::BinOp::Mul(_) => Some(Operator::Mul),
        syn::BinOp::Div(_) => Some(Operator::Div),
        syn::BinOp::Rem(_) => Some(Operator::Rem),
        syn::BinOp::Gt(_) => Some(Operator::Greater),
        syn::BinOp::Ge(_) => Some(Operator::GreaterEqual),
        syn::BinOp::Lt(_) => Some(Operator::Less),
        syn::BinOp::Le(_) => Some(Operator::LessEqual),
        syn::BinOp::Eq(_) => Some(Operator::Equal),
        syn::BinOp::Ne(_) => Some(Operator::NotEqual),
        syn::BinOp::And(_) => Some(Operator::And),
        syn::BinOp::Or(_) => Some(Operator::Or),
        syn::BinOp::AddAssign(_) => Some(Operator::AddAssign),
        syn::BinOp::SubAssign(_) => Some(Operator::SubAssign),
        syn::BinOp::MulAssign(_) => Some(Operator::MulAssign),
        syn::BinOp::DivAssign(_) => Some(Operator::DivAssign),
        syn::BinOp::RemAssign(_) => Some(Operator::RemAssign),
        syn::BinOp::BitAnd(_) => Some(Operator::BitAnd),
        syn::BinOp::BitOr(_) => Some(Operator::BitOr),
        syn::BinOp::BitXor(_) => Some(Operator::BitXor),
        syn::BinOp::Shl(_) => Some(Operator::Shl),
        syn::BinOp::Shr(_) => Some(Operator::Shr),
        _ => None,
    }
}

/// Maps a zero-argument method call onto its [`Operator`], or `None` when the
/// call is anything else.
///
/// **Preconditions:** the call must have **no arguments** and **no turbofish** —
/// a same-named method taking arguments or explicit generics is a different
/// method, and swapping its name would be a knowingly non-compiling mutant. The
/// arity rule is why `.expect("boom")` yields no site: it is not `.unwrap()` with
/// a message, it is a different method, and `expect(m) → unwrap_or_default()`
/// would leave a stray argument behind.
///
/// **No receiver type inference is attempted.** mutate4rust is a *syntactic*
/// tool: it has no type information, so a user-defined `is_some()` on an
/// unrelated type is mutated just like `Option::is_some`. That is accepted and
/// consistent with the rest of the scanner — the mutant either fails to compile
/// (scored Killed, A8) or is a genuine test of that predicate. The same holds for
/// `unwrap → unwrap_or_default`, whose real precondition (`T: Default`) is
/// undecidable without types.
fn method_call_operator(call: &syn::ExprMethodCall) -> Option<Operator> {
    if !call.args.is_empty() || call.turbofish.is_some() {
        return None;
    }
    match call.method.to_string().as_str() {
        "is_some" => Some(Operator::IsSome),
        "is_none" => Some(Operator::IsNone),
        "is_ok" => Some(Operator::IsOk),
        "is_err" => Some(Operator::IsErr),
        "unwrap" => Some(Operator::Unwrap),
        _ => None,
    }
}

/// The final path segment of a **one-argument plain-path call** — the shape
/// `Some(x)` / `Ok(x)` / `Option::Some(x)` must have to be a constructor site —
/// or `None` for any other call.
///
/// The preconditions are exactly the ones decidable **syntactically**, without
/// type information:
/// - **arity 1**: `Some()` or `Ok(a, b)` is not the constructor, whatever it is.
/// - **no turbofish**: `Some::<i32>(x) → None` would leave the generic arguments
///   dangling on a variant that takes none.
/// - **no `QSelf`**: `<T as Trait>::Some(x)` is a trait-associated function that
///   merely shares the name, not the `Option` constructor.
///
/// A *module*-qualified path (`Option::Some(2)`) **is** accepted — only the last
/// segment names the constructor, and both mappings are correct under it: the
/// whole-call span swallows the qualifier for `Some`, and replacing the `Ok`
/// identifier alone leaves `Result::Err(x)`.
///
/// Whether the path actually resolves to `core::option::Option::Some` is *not*
/// decidable here (A8): a user-defined `Some` is mutated too, and its mutant is
/// scored `Killed` if it fails to compile.
fn unary_constructor_segment(call: &syn::ExprCall) -> Option<&syn::PathSegment> {
    if call.args.len() != 1 {
        return None;
    }
    let syn::Expr::Path(path) = &*call.func else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    matches!(segment.arguments, syn::PathArguments::None).then_some(segment)
}

/// Maps an integer literal whose base-10 value is `0` or `1` onto the matching
/// constant [`Operator`]. `base10_digits` is radix-normalized by `syn`, so this
/// also matches e.g. `0x1` (value `1`); other values are out of scope.
///
/// A literal carrying a **float** suffix (`0f64`) also arrives here as a
/// [`syn::LitInt`]; it is not an integer constant and is filtered out — see
/// [`operators::is_integer_suffix`].
fn constant_operator(lit: &syn::LitInt) -> Option<Operator> {
    if !operators::is_integer_suffix(lit.suffix()) {
        return None;
    }
    match lit.base10_digits() {
        "0" => Some(Operator::Zero),
        "1" => Some(Operator::One),
        _ => None,
    }
}

/// Maps a float literal onto the matching constant [`Operator`], **only** for the
/// plain decimal spellings the mapping can rewrite without re-spelling the
/// author's literal.
///
/// The precondition lives in [`operators::plain_decimal_float_value`] and is
/// shared with the mapping, so a literal that would be rejected there is never
/// emitted as a site (precondition-gated emission). Exponent, point-less and
/// trailing-dot forms therefore yield no site.
fn float_constant_operator(lit: &syn::LitFloat) -> Option<Operator> {
    match operators::plain_decimal_float_value(lit.base10_digits()) {
        Some(0) => Some(Operator::FloatZero),
        Some(1) => Some(Operator::FloatOne),
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
    fn arithmetic_parity_operators() {
        // The three parity mappings must keep being discovered exactly as before
        // S4 added `/` and `%` (regression guard on the mutate4go parity set).
        let source = concat!(
            "fn add(a: i32, b: i32) -> i32 { a + b }\n",
            "fn sub(a: i32, b: i32) -> i32 { a - b }\n",
            "fn mul(a: i32, b: i32) -> i32 { a * b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 3);
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Arithmetic));
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
    fn arithmetic_idiomatic_completions_are_sites() {
        // `/` and `%` are S4 arithmetic completions — discovered, single-char spans.
        let source = concat!(
            "fn div(a: i32, b: i32) -> i32 { a / b }\n",
            "fn rem(a: i32, b: i32) -> i32 { a % b }\n",
        );
        let sites = scan(source);

        assert_eq!(sites.len(), 2);
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Arithmetic));
        assert_eq!(sites[0].operator, Operator::Div);
        assert_eq!(span_text(source, &sites[0]), "/");
        assert_eq!(sites[1].operator, Operator::Rem);
        assert_eq!(span_text(source, &sites[1]), "%");
        assert_eq!(
            sites
                .iter()
                .map(|s| s.function_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("div"), Some("rem")],
        );
    }

    #[test]
    fn compound_assignment_spans_cover_only_the_two_char_operator() {
        // The splice replaces the span verbatim, so the span must be the `+=`
        // token itself — never the whole `a += b` statement. Asserting both the
        // span text and its exact byte offsets locks that down.
        let source = concat!(
            "fn compound(mut a: i32, b: i32) {\n",
            "    a += b;\n",
            "    a -= b;\n",
            "    a *= b;\n",
            "    a /= b;\n",
            "    a %= b;\n",
            "}\n",
        );
        let sites = scan(source);

        let found: Vec<(Operator, &str)> = sites
            .iter()
            .map(|s| (s.operator, span_text(source, s)))
            .collect();
        assert_eq!(
            found,
            vec![
                (Operator::AddAssign, "+="),
                (Operator::SubAssign, "-="),
                (Operator::MulAssign, "*="),
                (Operator::DivAssign, "/="),
                (Operator::RemAssign, "%="),
            ],
        );
        assert!(
            sites
                .iter()
                .all(|s| s.kind() == SiteKind::CompoundAssignment
                    && s.function_id.as_deref() == Some("compound")),
        );
        // Byte-exact: each span is the operator's own offset pair, width 2.
        for (site, token) in sites.iter().zip(["+=", "-=", "*=", "/=", "%="]) {
            let expected_start = source.find(token).expect("token is in the fixture");
            assert_eq!(site.byte_span, expected_start..expected_start + 2);
        }
        assert_eq!(
            sites.iter().map(|s| s.line).collect::<Vec<_>>(),
            vec![2, 3, 4, 5, 6],
        );
    }

    #[test]
    fn bitwise_and_shift_operators_are_sites() {
        // S6/T13a: `& | ^ << >>` are in scope, each spanning exactly its own
        // token. Operands are non-literal params so no constant/boolean site can
        // sneak in and skew the count.
        let source = concat!(
            "fn bit_and(a: i32, b: i32) -> i32 { a & b }\n",
            "fn bit_or(a: i32, b: i32) -> i32 { a | b }\n",
            "fn bit_xor(a: i32, b: i32) -> i32 { a ^ b }\n",
            "fn shl(a: i32, b: i32) -> i32 { a << b }\n",
            "fn shr(a: i32, b: i32) -> i32 { a >> b }\n",
        );
        let sites = scan(source);

        assert!(sites.iter().all(|s| s.kind() == SiteKind::Bitwise));
        assert_eq!(
            sites
                .iter()
                .map(|s| (s.operator, span_text(source, s)))
                .collect::<Vec<_>>(),
            vec![
                (Operator::BitAnd, "&"),
                (Operator::BitOr, "|"),
                (Operator::BitXor, "^"),
                (Operator::Shl, "<<"),
                (Operator::Shr, ">>"),
            ],
        );
    }

    #[test]
    fn bitwise_assign_forms_emit_no_sites() {
        // The taxonomy lists `& | ^ << >>` only — the assign forms are NOT in
        // T13a scope and must still yield nothing.
        let source = concat!(
            "fn bit_compound(mut a: i32, b: i32) {\n",
            "    a &= b;\n",
            "    a |= b;\n",
            "    a ^= b;\n",
            "    a <<= b;\n",
            "    a >>= b;\n",
            "}\n",
        );

        assert!(
            scan(source).is_empty(),
            "bitwise assign forms are out of T13a scope",
        );
    }

    #[test]
    fn predicate_method_calls_span_the_method_name_only() {
        // The splice replaces the span verbatim, so the span must be the method
        // identifier — not the receiver, the dot, or the `()`.
        let source = concat!(
            "fn a(x: Option<i32>) -> bool { x.is_some() }\n",
            "fn b(x: Option<i32>) -> bool { x.is_none() }\n",
            "fn c(x: Result<i32, ()>) -> bool { x.is_ok() }\n",
            "fn d(x: Result<i32, ()>) -> bool { x.is_err() }\n",
        );
        let sites = scan(source);

        assert!(sites.iter().all(|s| s.kind() == SiteKind::PredicateMethod));
        assert_eq!(
            sites
                .iter()
                .map(|s| (s.operator, span_text(source, s)))
                .collect::<Vec<_>>(),
            vec![
                (Operator::IsSome, "is_some"),
                (Operator::IsNone, "is_none"),
                (Operator::IsOk, "is_ok"),
                (Operator::IsErr, "is_err"),
            ],
        );
    }

    /// A predicate call is discovered wherever it sits — as the outer call of a
    /// chain, or nested inside another call's argument — and the span is always
    /// that call's own method identifier, never the receiver's.
    #[test]
    fn nested_and_chained_predicate_calls_span_their_own_method_name() {
        let source = concat!(
            "fn a(x: S) -> bool { x.b().is_some() }\n",
            "fn b(x: S) -> bool { foo(x.is_ok()).is_some() }\n",
            "fn c(x: S) -> bool { x.is_some().is_none() }\n",
        );
        let sites = scan(source);

        assert_eq!(
            sites
                .iter()
                .map(|s| (s.operator, span_text(source, s), s.line))
                .collect::<Vec<_>>(),
            vec![
                // The receiver `x.b()` is not a predicate call, so the only site
                // on line 1 is the outer `is_some`.
                (Operator::IsSome, "is_some", 1),
                // The visitor records a call before descending into it, so the
                // outer call comes first and the nested argument second — two
                // distinct spans, neither of them the receiver's.
                (Operator::IsSome, "is_some", 2),
                (Operator::IsOk, "is_ok", 2),
                // A predicate call *as* a receiver is itself a site, and each
                // call's span is its own name.
                (Operator::IsNone, "is_none", 3),
                (Operator::IsSome, "is_some", 3),
            ],
        );
    }

    #[test]
    fn a_predicate_method_with_arguments_or_a_turbofish_is_not_a_site() {
        // Preconditions: zero arguments and no turbofish. A same-named method
        // that takes either is a different method, and renaming it would be a
        // knowingly non-compiling mutant — so no site is emitted.
        //
        // The control below is the identical call *without* the argument /
        // turbofish, proving the precondition is what suppresses the site.
        assert!(
            scan("fn f(x: S) -> bool { x.is_some(2) }\n").is_empty(),
            "an argument disqualifies the call",
        );
        assert!(
            scan("fn f(x: S) -> bool { x.is_ok::<i32>() }\n").is_empty(),
            "a turbofish disqualifies the call",
        );
        assert_eq!(
            scan("fn f(x: S) -> bool { x.is_some() }\n")
                .iter()
                .map(|s| s.operator)
                .collect::<Vec<_>>(),
            vec![Operator::IsSome],
            "control: the same call without either is a site",
        );
    }

    #[test]
    fn an_unrelated_zero_argument_method_is_not_a_site() {
        assert!(
            scan("fn f(x: S) -> bool { x.is_empty() }\n").is_empty(),
            "only the four taxonomy predicates are sites",
        );
    }

    #[test]
    fn a_float_suffixed_integer_literal_is_not_a_constant_site() {
        // `0f64` parses as a *suffixed `LitInt`*, but it is a float literal: the
        // integer mapping cannot rewrite it, and the point-less float spelling
        // is deliberately out of scope. Emitting it would have produced a mutant
        // whose mapping fails mid-run. The control proves the float **suffix** is
        // what disqualifies it, not the value or the literal kind.
        assert!(scan("fn f() -> f64 { 0f64 }\n").is_empty());
        assert!(scan("fn f() -> f32 { 1f32 }\n").is_empty());
        assert_eq!(
            scan("fn f() -> u8 { 0u8 }\n")
                .iter()
                .map(|s| s.operator)
                .collect::<Vec<_>>(),
            vec![Operator::Zero],
            "control: an integer-suffixed literal is still a site",
        );
    }

    #[test]
    fn plain_decimal_float_constants_are_sites() {
        // S6/T13a: `0.0`/`1.0` are discovered, span covering the WHOLE literal
        // (suffix included) so the mapping can preserve it.
        let source = concat!(
            "fn zero() -> f64 { 0.0 }\n",
            "fn one() -> f32 { 1.0f32 }\n",
            "fn also_zero() -> f32 { 0.0_f32 }\n",
        );
        let sites = scan(source);

        assert!(sites.iter().all(|s| s.kind() == SiteKind::FloatConstant));
        assert_eq!(
            sites
                .iter()
                .map(|s| (s.operator, span_text(source, s)))
                .collect::<Vec<_>>(),
            vec![
                (Operator::FloatZero, "0.0"),
                (Operator::FloatOne, "1.0f32"),
                (Operator::FloatZero, "0.0_f32"),
            ],
        );
    }

    #[test]
    fn float_spellings_outside_the_precondition_emit_no_sites() {
        // Precondition-gated emission: rewriting these would mean re-spelling a
        // literal the author did not write, so no site is emitted at all. Each
        // is paired with its in-scope control in the test above.
        let source = concat!(
            "fn a() -> f64 { 1e0 }\n",
            "fn b() -> f64 { 0.0e0 }\n",
            "fn c() -> f64 { 0f64 }\n",
            "fn d() -> f64 { 1.5 }\n",
            "fn e() -> f64 { 2.0 }\n",
            "fn f() -> f64 { 10.0 }\n",
        );

        assert!(
            scan(source).is_empty(),
            "only plain decimal 0/1 floats are sites, got {:?}",
            scan(source),
        );
    }

    #[test]
    fn s4_spans_and_mappings_compose_into_the_expected_mutant_source() {
        // Composition proof for every new S4 row: the span the scanner reports,
        // replaced by the operator's mapping, yields exactly these bytes.
        for (source, expected) in [
            (
                "fn f(a: i32, b: i32) -> i32 { a / b }\n",
                "fn f(a: i32, b: i32) -> i32 { a * b }\n",
            ),
            (
                "fn f(a: i32, b: i32) -> i32 { a % b }\n",
                "fn f(a: i32, b: i32) -> i32 { a * b }\n",
            ),
            (
                "fn f(mut a: i32, b: i32) { a += b; }\n",
                "fn f(mut a: i32, b: i32) { a -= b; }\n",
            ),
            (
                "fn f(mut a: i32, b: i32) { a -= b; }\n",
                "fn f(mut a: i32, b: i32) { a += b; }\n",
            ),
            (
                "fn f(mut a: i32, b: i32) { a *= b; }\n",
                "fn f(mut a: i32, b: i32) { a /= b; }\n",
            ),
            (
                "fn f(mut a: i32, b: i32) { a /= b; }\n",
                "fn f(mut a: i32, b: i32) { a *= b; }\n",
            ),
            (
                "fn f(mut a: i32, b: i32) { a %= b; }\n",
                "fn f(mut a: i32, b: i32) { a *= b; }\n",
            ),
        ] {
            let sites = scan(source);
            assert_eq!(sites.len(), 1, "one site expected in `{source}`");
            let site = &sites[0];
            let replacement = crate::operators::replacement(site.operator, span_text(source, site))
                .expect("mapping should succeed");
            let mutated = format!(
                "{}{replacement}{}",
                &source[..site.byte_span.start],
                &source[site.byte_span.end..],
            );
            assert_eq!(mutated, expected);
        }
    }

    #[test]
    fn s6_spans_and_mappings_compose_into_the_expected_mutant_source() {
        // Composition proof for every new T13a row: the span the scanner
        // reports, replaced by the operator's mapping, yields exactly these
        // bytes — the mutant a real run would write to disk.
        for (source, expected) in [
            (
                "fn f(a: i32, b: i32) -> i32 { a & b }\n",
                "fn f(a: i32, b: i32) -> i32 { a | b }\n",
            ),
            (
                "fn f(a: i32, b: i32) -> i32 { a | b }\n",
                "fn f(a: i32, b: i32) -> i32 { a & b }\n",
            ),
            (
                "fn f(a: i32, b: i32) -> i32 { a ^ b }\n",
                "fn f(a: i32, b: i32) -> i32 { a & b }\n",
            ),
            (
                "fn f(a: i32, b: i32) -> i32 { a << b }\n",
                "fn f(a: i32, b: i32) -> i32 { a >> b }\n",
            ),
            (
                "fn f(a: i32, b: i32) -> i32 { a >> b }\n",
                "fn f(a: i32, b: i32) -> i32 { a << b }\n",
            ),
            (
                "fn f(x: Option<i32>) -> bool { x.is_some() }\n",
                "fn f(x: Option<i32>) -> bool { x.is_none() }\n",
            ),
            (
                "fn f(x: Option<i32>) -> bool { x.is_none() }\n",
                "fn f(x: Option<i32>) -> bool { x.is_some() }\n",
            ),
            (
                "fn f(x: Result<i32, ()>) -> bool { x.is_ok() }\n",
                "fn f(x: Result<i32, ()>) -> bool { x.is_err() }\n",
            ),
            (
                "fn f(x: Result<i32, ()>) -> bool { x.is_err() }\n",
                "fn f(x: Result<i32, ()>) -> bool { x.is_ok() }\n",
            ),
            ("fn f() -> f64 { 0.0 }\n", "fn f() -> f64 { 1.0 }\n"),
            ("fn f() -> f64 { 1.0 }\n", "fn f() -> f64 { 0.0 }\n"),
            // The suffix survives: dropping `f32` would change the mutant's type.
            ("fn f() -> f32 { 1.0f32 }\n", "fn f() -> f32 { 0.0f32 }\n"),
            ("fn f() -> f32 { 0.0_f32 }\n", "fn f() -> f32 { 1.0f32 }\n"),
        ] {
            let sites = scan(source);
            assert_eq!(sites.len(), 1, "one site expected in `{source}`");
            let site = &sites[0];
            let replacement = crate::operators::replacement(site.operator, span_text(source, site))
                .expect("mapping should succeed");
            let mutated = format!(
                "{}{replacement}{}",
                &source[..site.byte_span.start],
                &source[site.byte_span.end..],
            );
            assert_eq!(mutated, expected);
        }
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
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Comparison));
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
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Equality));
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
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Logical));
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
        assert!(sites.iter().all(|s| s.kind() == SiteKind::BooleanLiteral));
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
        assert!(sites.iter().all(|s| s.kind() == SiteKind::Constant));
        assert_eq!(sites[0].operator, Operator::Zero);
        assert_eq!(span_text(source, &sites[0]), "0");
        assert_eq!(sites[1].operator, Operator::One);
        assert_eq!(span_text(source, &sites[1]), "1");
    }

    /// Sites inside the file's own `#[cfg(test)] mod tests` are never discovered
    /// — the deliberate divergence from upstream recorded in the module header.
    ///
    /// The identical body **without** the attribute is scanned, so this proves
    /// the attribute is what suppresses discovery, not the module nesting or an
    /// unrelated scanner gap.
    #[test]
    fn sites_inside_a_cfg_test_module_are_not_discovered() {
        const BODY: &str = concat!(
            "fn production(a: i32, b: i32) -> i32 { a + b }\n",
            "mod tests {\n",
            "    fn helper(c: i32, d: i32) -> i32 { c * d }\n",
            "}\n",
        );

        let without_attribute = scan(BODY);
        assert_eq!(
            without_attribute
                .iter()
                .map(|s| s.operator)
                .collect::<Vec<_>>(),
            vec![Operator::Add, Operator::Mul],
            "control: the same body is scanned when not marked test-only",
        );

        let sites = scan(&BODY.replace("mod tests {", "#[cfg(test)]\nmod tests {"));
        assert_eq!(
            sites.iter().map(|s| s.operator).collect::<Vec<_>>(),
            vec![Operator::Add],
            "the `c * d` site inside `#[cfg(test)] mod tests` must be skipped",
        );
    }

    #[test]
    fn a_cfg_test_function_is_not_discovered() {
        let sites = scan(concat!(
            "fn production(a: i32, b: i32) -> i32 { a + b }\n",
            "#[cfg(test)]\n",
            "fn only_in_tests(c: i32, d: i32) -> i32 { c * d }\n",
        ));

        assert_eq!(
            sites.iter().map(|s| s.operator).collect::<Vec<_>>(),
            vec![Operator::Add],
        );
    }

    /// Only the literal `#[cfg(test)]` suppresses discovery: a composite
    /// predicate still compiles in a non-test build, so its sites stay in scope.
    #[test]
    fn a_composite_cfg_predicate_is_still_scanned() {
        let sites = scan(concat!(
            "#[cfg(any(test, feature = \"extra\"))]\n",
            "mod maybe {\n",
            "    fn f(c: i32, d: i32) -> i32 { c * d }\n",
            "}\n",
        ));

        assert_eq!(
            sites.iter().map(|s| s.operator).collect::<Vec<_>>(),
            vec![Operator::Mul],
        );
    }

    /// The holes the `#[cfg(test)]` gate closes, one row per *position* an
    /// attribute can sit in and still suppress a site.
    ///
    /// At T13a this covered the four item enums (`visit_item` /
    /// `visit_trait_item` / `visit_impl_item` / `visit_foreign_item`): before
    /// them, `#[cfg(test)]` was checked only on free functions, impl methods,
    /// impl blocks and modules, so a test-only **trait**, **trait method**, or
    /// module-level **`const`/`static`** still contributed sites. T13b renamed
    /// the test from `..._every_item_kind_...` and added the **`Stmt::Local`**
    /// and **`Arm`** rows for its two new gates.
    ///
    /// Each row pairs the attributed construct with the **identical body lacking
    /// the attribute**, so a row can never pass by asserting emptiness against
    /// emptiness: the control proves the fixture really does hold a site, and
    /// only then does the attributed form have to suppress it. A passing
    /// assertion therefore means the *attribute* suppressed discovery — not that
    /// the scanner never looked there. This is the enforcement for a foreign
    /// `#[non_exhaustive]` domain that neither construction nor the compiler can
    /// provide (design: "make the test the enforcement"), and it is the artifact
    /// a `syn` bump must be re-run against.
    ///
    /// The two T13a **foreign-item** rows were removed at T13b: once
    /// [`Visit::visit_type`] stopped recursing, their controls held no site
    /// either, so both sides were empty and the rows asserted nothing. The
    /// emptiness they used to imply is now pinned directly by
    /// [`a_foreign_item_holds_no_site_with_or_without_the_attribute`].
    #[test]
    fn cfg_test_suppresses_every_position_that_can_hold_a_site() {
        for (attributed, control, what) in [
            (
                concat!(
                    "#[cfg(test)]\n",
                    "trait T {\n",
                    "    fn f(a: i32, b: i32) -> i32 { a + b }\n",
                    "}\n",
                ),
                concat!(
                    "trait T {\n",
                    "    fn f(a: i32, b: i32) -> i32 { a + b }\n",
                    "}\n",
                ),
                "a test-only trait",
            ),
            (
                concat!(
                    "trait T {\n",
                    "    #[cfg(test)]\n",
                    "    fn f(a: i32, b: i32) -> i32 { a + b }\n",
                    "}\n",
                ),
                concat!(
                    "trait T {\n",
                    "    fn f(a: i32, b: i32) -> i32 { a + b }\n",
                    "}\n",
                ),
                "a test-only trait method",
            ),
            (
                "#[cfg(test)]\nconst K: i32 = 1;\n",
                "const K: i32 = 1;\n",
                "a test-only module-level const",
            ),
            (
                "#[cfg(test)]\nstatic S: i32 = 0;\n",
                "static S: i32 = 0;\n",
                "a test-only module-level static",
            ),
            (
                concat!(
                    "struct S;\n",
                    "impl S {\n",
                    "    #[cfg(test)]\n",
                    "    const C: i32 = 1;\n",
                    "}\n",
                ),
                concat!(
                    "struct S;\n",
                    "impl S {\n",
                    "    const C: i32 = 1;\n",
                    "}\n"
                ),
                "a test-only associated const",
            ),
            (
                concat!(
                    "fn outer() -> i32 {\n",
                    "    #[cfg(test)]\n",
                    "    const K: i32 = 1;\n",
                    "    2\n",
                    "}\n",
                ),
                concat!(
                    "fn outer() -> i32 {\n",
                    "    const K: i32 = 1;\n",
                    "    2\n",
                    "}\n",
                ),
                "a test-only const inside a function body",
            ),
            (
                concat!(
                    "fn outer(a: i32, b: i32) -> i32 {\n",
                    "    #[cfg(test)]\n",
                    "    let _k = a + b;\n",
                    "    2\n",
                    "}\n",
                ),
                concat!(
                    "fn outer(a: i32, b: i32) -> i32 {\n",
                    "    let _k = a + b;\n",
                    "    2\n",
                    "}\n",
                ),
                "a test-only `let` statement",
            ),
            (
                concat!(
                    "fn outer(x: i32) -> i32 {\n",
                    "    match x {\n",
                    "        #[cfg(test)]\n",
                    "        y => y + 1,\n",
                    "        _ => x,\n",
                    "    }\n",
                    "}\n",
                ),
                concat!(
                    "fn outer(x: i32) -> i32 {\n",
                    "    match x {\n",
                    "        y => y + 1,\n",
                    "        _ => x,\n",
                    "    }\n",
                    "}\n",
                ),
                "a test-only `match` arm",
            ),
        ] {
            assert!(
                !scan(control).is_empty(),
                "control: {what} must be scanned without the attribute",
            );
            assert!(
                scan(attributed).is_empty(),
                "{what} must contribute no site, got {:?}",
                scan(attributed),
            );
        }
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

    /// Validation and discovery agree on what "valid Rust" means — the pre-flight
    /// check cannot accept a file the scan would then reject.
    #[test]
    fn validation_accepts_exactly_what_discovery_parses() {
        use super::validate_source;

        assert!(validate_source("fn f(a: i32) -> i32 { a + 1 }\n").is_ok());
        assert!(validate_source("fn broken(").is_err());
        assert!(validate_source("this is not rust").is_err());
        // Empty input is a valid (empty) Rust file, exactly as `scan_source` sees it.
        assert!(validate_source("").is_ok());
        assert!(scan_source("").is_ok());
    }

    /// The mutant source a real run would write: the site's span replaced by its
    /// operator's mapping, spliced over the pristine bytes.
    fn splice(source: &str, site: &Site) -> String {
        let replacement = crate::operators::replacement(site.operator, span_text(source, site))
            .expect("mapping should succeed");
        format!(
            "{}{replacement}{}",
            &source[..site.byte_span.start],
            &source[site.byte_span.end..],
        )
    }

    /// The one [`Operator::ArmGuard`] site in `source`, asserting there is
    /// exactly one so a fixture can never silently grow a second guard.
    fn arm_guard_site(source: &str) -> Site {
        let mut guards = scan(source)
            .into_iter()
            .filter(|site| site.operator == Operator::ArmGuard);
        let site = guards.next().expect("an arm guard site");
        assert!(guards.next().is_none(), "exactly one arm guard expected");
        site
    }

    /// Sites live in expressions, never in types: `visit_type` does not recurse.
    ///
    /// Not tautological — the control scans the *same* literals in **expression**
    /// position and requires them to be sites, so this cannot pass merely because
    /// `0`/`1` went out of scope everywhere. Restoring the recursion makes the
    /// five type-position literals below into five sites and fails the assertion.
    #[test]
    fn no_site_is_emitted_inside_a_type() {
        // Every `0`/`1` here sits in type position: a field type, a parameter
        // type, a return type, a type alias, and a static's type.
        let types = concat!(
            "struct S {\n",
            "    a: [u8; 1],\n",
            "}\n",
            "fn f(x: [i32; 1]) -> [i32; 1] {\n",
            "    x\n",
            "}\n",
            "type A = [u8; 0];\n",
            "static B: [u8; 1] = [7u8];\n",
        );
        let control = concat!("fn g() -> i32 { 1 }\n", "fn h() -> i32 { 0 }\n");

        assert_eq!(
            scan(control).len(),
            2,
            "control: the same literals in expression position must be sites",
        );
        assert!(
            scan(types).is_empty(),
            "a literal in type position must contribute no site, got {:?}",
            scan(types),
        );
    }

    /// An array **repeat expression** is an expression and stays in scope, even
    /// though it looks like an array type.
    ///
    /// Both halves are sites — the element and the repeat length — while the
    /// identical literal in **return-type** position contributes nothing. That
    /// asymmetry is the whole point: 2 sites, not 3. A recursing `visit_type`
    /// scores 3 here.
    #[test]
    fn array_repeat_expressions_stay_in_scope() {
        let source = "fn f() -> [u8; 1] { [0u8; 1] }\n";
        let sites = scan(source);

        assert_eq!(
            sites.iter().map(|s| s.operator).collect::<Vec<_>>(),
            vec![Operator::Zero, Operator::One],
            "the repeat element and length are sites; the return type is not",
        );
        assert_eq!(span_text(source, &sites[0]), "0u8");
        assert_eq!(span_text(source, &sites[1]), "1");
    }

    /// A foreign item holds no site at all now that `visit_type` does not
    /// recurse — with **or** without `#[cfg(test)]`.
    ///
    /// This is what makes [`foreign_item_attrs`]'s gate vacuous, and it is not a
    /// vacuous test: it pins the emptiness itself, so it fails the moment
    /// `visit_type` starts recursing again and the array lengths below become
    /// sites.
    #[test]
    fn a_foreign_item_holds_no_site_with_or_without_the_attribute() {
        for (source, what) in [
            (
                concat!("unsafe extern \"C\" {\n", "    static X: [u8; 1];\n", "}\n"),
                "a foreign static",
            ),
            (
                concat!(
                    "unsafe extern \"C\" {\n",
                    "    #[cfg(test)]\n",
                    "    static X: [u8; 1];\n",
                    "}\n",
                ),
                "a test-only foreign static",
            ),
            (
                concat!("unsafe extern \"C\" {\n", "    fn g() -> [u8; 1];\n", "}\n"),
                "a foreign function",
            ),
            (
                concat!(
                    "unsafe extern \"C\" {\n",
                    "    #[cfg(test)]\n",
                    "    fn g() -> [u8; 1];\n",
                    "}\n",
                ),
                "a test-only foreign function",
            ),
        ] {
            assert!(
                scan(source).is_empty(),
                "{what} must contribute no site, got {:?}",
                scan(source),
            );
        }
    }

    /// Each structural S6 site spans **exactly** the text its mapping replaces —
    /// no more, no less. The spans differ per operator by design, so this pins
    /// each one separately.
    #[test]
    fn structural_operator_spans_cover_exactly_what_the_mutation_replaces() {
        for (source, operator, expected, what) in [
            (
                "fn f(a: i32) -> Option<i32> { Some(a) }\n",
                Operator::SomeCall,
                "Some(a)",
                "`Some(x)` spans the whole call, because `None` discards the value",
            ),
            (
                "fn f() -> Option<i32> { Option::Some(2) }\n",
                Operator::SomeCall,
                "Option::Some(2)",
                "a module-qualified `Some` still spans the whole call",
            ),
            (
                "fn f(a: i32) -> Result<i32, ()> { Ok(a) }\n",
                Operator::OkCall,
                "Ok",
                "`Ok(x)` spans the identifier only, so the value survives into `Err(x)`",
            ),
            (
                "fn f(x: Option<i32>) -> i32 { x.unwrap() }\n",
                Operator::Unwrap,
                "unwrap",
                "`.unwrap()` spans the method name only",
            ),
            (
                "fn f(x: Option<i32>) -> Option<i32> { x?; x }\n",
                Operator::Try,
                "?",
                "`?` spans the token only, leaving the receiver untouched",
            ),
            (
                "fn f(x: i32) -> i32 { match x { y if y > 7 => y, _ => x } }\n",
                Operator::ArmGuard,
                "if y > 7",
                "an arm guard spans `if <guard>` whole",
            ),
        ] {
            let sites = scan(source);
            let site = sites
                .iter()
                .find(|site| site.operator == operator)
                .unwrap_or_else(|| panic!("{operator:?} not discovered in `{source}`"));
            assert_eq!(span_text(source, site), expected, "{what}");
        }
    }

    /// The syntactically-decidable preconditions really are enforced.
    ///
    /// Each row pairs a form that must emit **no** site with the near-miss
    /// **control** that must emit one, so a row cannot pass because the scanner
    /// never looked at that shape at all.
    #[test]
    fn structural_forms_outside_the_precondition_emit_no_sites() {
        for (source, control, what) in [
            (
                "fn f(a: i32) -> Option<i32> { Some::<i32>(a) }\n",
                "fn f(a: i32) -> Option<i32> { Some(a) }\n",
                "a turbofished `Some` would leave generics on a variant taking none",
            ),
            (
                "fn f(a: i32) -> i32 { <S as T>::Some(a) }\n",
                "fn f(a: i32) -> Option<i32> { Some(a) }\n",
                "a `QSelf`-qualified path merely shares the name",
            ),
            (
                "fn f(a: i32, b: i32) -> R { Ok(a, b) }\n",
                "fn f(a: i32) -> Result<i32, ()> { Ok(a) }\n",
                "a two-argument `Ok` is not the constructor",
            ),
            (
                "fn f(x: Option<i32>) -> i32 { x.expect(\"boom\") }\n",
                "fn f(x: Option<i32>) -> i32 { x.unwrap() }\n",
                "`.expect(m)` takes an argument, so it is not `.unwrap()`",
            ),
            (
                "fn f(x: i32) -> i32 { match x { y => y, _ => x } }\n",
                "fn f(x: i32, c: bool) -> i32 { match x { y if c => y, _ => x } }\n",
                "an arm without a guard has nothing to drop",
            ),
        ] {
            assert!(
                !scan(control).is_empty(),
                "control: {what} — the in-precondition form must be a site",
            );
            assert!(scan(source).is_empty(), "{what}, got {:?}", scan(source),);
        }
    }

    /// Composition proof for every structural S6 row: the span the scanner
    /// reports, replaced by the operator's mapping, yields exactly these bytes —
    /// the mutant a real run would write to disk.
    #[test]
    fn s6_structural_spans_and_mappings_compose_into_the_expected_mutant_source() {
        for (source, expected) in [
            (
                "fn f(a: i32) -> Option<i32> { Some(a) }\n",
                "fn f(a: i32) -> Option<i32> { None }\n",
            ),
            (
                "fn f() -> Option<i32> { Option::Some(2) }\n",
                "fn f() -> Option<i32> { None }\n",
            ),
            (
                "fn f(a: i32) -> Result<i32, ()> { Ok(a) }\n",
                "fn f(a: i32) -> Result<i32, ()> { Err(a) }\n",
            ),
            (
                "fn f(x: Option<i32>) -> i32 { x.unwrap() }\n",
                "fn f(x: Option<i32>) -> i32 { x.unwrap_or_default() }\n",
            ),
            (
                "fn f(x: Option<i32>) -> Option<i32> { x?; x }\n",
                "fn f(x: Option<i32>) -> Option<i32> { x.unwrap(); x }\n",
            ),
            // Dropping the guard leaves the pattern and body untouched — and the
            // space the `if` used to occupy, which is harmless to `rustc`.
            (
                "fn f(x: i32, c: bool) -> i32 { match x { y if c => y, _ => x } }\n",
                "fn f(x: i32, c: bool) -> i32 { match x { y  => y, _ => x } }\n",
            ),
        ] {
            let sites = scan(source);
            assert_eq!(sites.len(), 1, "one site expected in `{source}`");
            assert_eq!(splice(source, &sites[0]), expected);
        }
    }

    /// The **assembled** arm-guard span (the one construction with no single
    /// `syn::Span`) covers a multi-token guard whole, across every shape a guard
    /// can take.
    ///
    /// Per row: the span text is the entire guard; the splice is exactly the
    /// expected source; and that source **parses**. The parse check is what gives
    /// the test teeth — a span truncated to any prefix of the guard leaves the
    /// remaining tokens stranded between the pattern and the `=>`, which
    /// `syn::parse_file` genuinely rejects. `assert_eq!` runs first so a failure
    /// reports the wrong bytes rather than a bare parse error.
    #[test]
    fn the_assembled_arm_guard_span_covers_a_multi_token_guard_whole() {
        for (source, guard, expected, what) in [
            (
                "fn f(a: i32, b: i32, x: i32) -> i32 { match x { y if a > b => y, _ => x } }\n",
                "if a > b",
                "fn f(a: i32, b: i32, x: i32) -> i32 { match x { y  => y, _ => x } }\n",
                "a binary comparison",
            ),
            (
                "fn f(s: &str, x: i32) -> i32 { match x { y if s.starts_with('a') => y, _ => x } }\n",
                "if s.starts_with('a')",
                "fn f(s: &str, x: i32) -> i32 { match x { y  => y, _ => x } }\n",
                "a method call",
            ),
            (
                "fn f(a: i32, b: i32, x: i32) -> i32 { match x { y if (a > b) => y, _ => x } }\n",
                "if (a > b)",
                "fn f(a: i32, b: i32, x: i32) -> i32 { match x { y  => y, _ => x } }\n",
                "a parenthesised guard",
            ),
            (
                "fn f(a: bool, b: bool, x: i32) -> i32 { match x { y if a && b => y, _ => x } }\n",
                "if a && b",
                "fn f(a: bool, b: bool, x: i32) -> i32 { match x { y  => y, _ => x } }\n",
                "an `&&`-chained guard",
            ),
        ] {
            let site = arm_guard_site(source);
            assert_eq!(span_text(source, &site), guard, "{what}: span text");

            let mutated = splice(source, &site);
            assert_eq!(mutated, expected, "{what}: mutant source");
            assert!(
                syn::parse_file(&mutated).is_ok(),
                "{what}: the mutant must be valid Rust",
            );
        }
    }

    /// O3: a multi-token guard emits **both** its own `ArmGuard` site and the
    /// operator sites nested inside it, with overlapping byte spans.
    ///
    /// This is deliberate. Mutants are applied one at a time, each spliced over
    /// pristine source, so overlapping sites never collide — they are two
    /// independent mutants. Combining spans is T13c.
    #[test]
    fn a_guard_emits_its_own_site_and_the_operator_sites_nested_inside_it() {
        let source =
            "fn f(a: i32, b: i32, x: i32) -> i32 { match x { y if a > b => y, _ => x } }\n";
        let sites = scan(source);

        assert_eq!(
            sites.iter().map(|s| s.operator).collect::<Vec<_>>(),
            vec![Operator::ArmGuard, Operator::Greater],
        );

        let guard = &sites[0];
        let nested = &sites[1];
        assert_eq!(span_text(source, guard), "if a > b");
        assert_eq!(span_text(source, nested), ">");
        assert!(
            guard.byte_span.start < nested.byte_span.start
                && nested.byte_span.end < guard.byte_span.end,
            "the operator span {:?} must sit strictly inside the guard span {:?}",
            nested.byte_span,
            guard.byte_span,
        );
    }

    /// `Some`/`Ok` in **pattern** position bind a value; they construct nothing,
    /// so mutating them would be nonsense. Each row is paired with the same
    /// constructor in **expression** position as a control.
    #[test]
    fn constructors_in_pattern_position_are_not_sites() {
        for (pattern, control, what) in [
            (
                "fn f(x: Option<i32>, d: i32) -> i32 { match x { Some(v) => v, None => d } }\n",
                "fn f(v: i32) -> Option<i32> { Some(v) }\n",
                "`Some(v)` in a match arm",
            ),
            (
                "fn f(x: Result<i32, ()>, d: i32) -> i32 { match x { Ok(v) => v, Err(_) => d } }\n",
                "fn f(v: i32) -> Result<i32, ()> { Ok(v) }\n",
                "`Ok(v)` in a match arm",
            ),
            (
                "fn f(x: Option<i32>, d: i32) -> i32 { if let Some(v) = x { v } else { d } }\n",
                "fn f(v: i32) -> Option<i32> { Some(v) }\n",
                "`Some(v)` in an `if let`",
            ),
        ] {
            assert!(
                !scan(control).is_empty(),
                "control: {what} — the expression form must be a site",
            );
            assert!(
                scan(pattern).is_empty(),
                "{what} must contribute no site, got {:?}",
                scan(pattern),
            );
        }
    }

    /// Sources exercising every shape the scanner can emit a site for, plus the
    /// near-miss spellings it must **refuse** to emit.
    ///
    /// Deliberately includes forms whose mapping would *fail* if the scanner
    /// wrongly emitted them — `0f64` is a `syn::LitInt` with a float suffix, and
    /// emitting it as an integer constant makes `replacement` reject the token.
    const TOTALITY_CORPUS: &[&str] = &[
        "fn f(a: i32, b: i32) -> i32 { a + b - a * b / a % b }\n",
        "fn f(mut a: i32, b: i32) { a += b; a -= b; a *= b; a /= b; a %= b; }\n",
        "fn f(a: i32, b: i32) -> bool { a > b && a >= b || a < b && a <= b }\n",
        "fn f(a: i32, b: i32) -> bool { a == b || a != b }\n",
        "fn f() -> bool { true || false }\n",
        "fn f() -> i32 { 0 + 1 }\n",
        "fn f() -> i32 { 0x1 + 0b0 + 1u8 as i32 + 0usize as i32 }\n",
        "fn f() -> f64 { 0.0 + 1.0 }\n",
        "fn f() -> f32 { 0.0f32 + 1.0_f32 }\n",
        // Refused spellings: a float-suffixed integer literal, and the float
        // forms outside the plain-decimal precondition.
        "fn f() -> f64 { 0f64 }\n",
        "fn f() -> f64 { 1e0 + 0. + 1.0e3 }\n",
        "fn f(a: i32, b: i32) -> i32 { a & b | a ^ b }\n",
        "fn f(a: i32, b: i32) -> i32 { (a << b) + (a >> b) }\n",
        "fn f(o: Option<i32>, r: Result<i32, ()>) -> bool { o.is_some() && o.is_none() }\n",
        "fn f(r: Result<i32, ()>) -> bool { r.is_ok() && r.is_err() }\n",
        "fn f(a: i32) -> Option<i32> { Some(a) }\n",
        "fn f() -> Option<i32> { Option::Some(2) }\n",
        "fn f(a: i32) -> Result<i32, ()> { Ok(a) }\n",
        "fn f(x: Option<i32>) -> i32 { x.unwrap() }\n",
        "fn f(x: Option<i32>) -> Option<i32> { Some(x?) }\n",
        "fn f(x: i32) -> i32 { match x { y if y > 7 => y, _ => x } }\n",
        "fn f() -> [u8; 1] { [0u8; 1] }\n",
        "fn f(x: Option<i32>) -> i32 { x.expect(\"boom\") }\n",
        "unsafe extern \"C\" {\n    static X: [u8; 1];\n}\n",
    ];

    /// **Totality of the mapping over what the scanner emits.**
    ///
    /// Two properties in one, and they pull against each other, which is the
    /// point:
    /// - *Over-emission fails loudly*: every site the scanner emits must map
    ///   successfully. A site whose token `replacement` rejects — a `0f64`
    ///   emitted as an integer constant, say — fails here rather than surviving
    ///   to become a run-time mapping error mid-run.
    /// - *Under-emission fails loudly*: the corpus must collectively emit **every**
    ///   operator in [`Operator::ALL`]. Silently dropping a shape from discovery
    ///   would otherwise just make coverage rows quietly disappear.
    #[test]
    fn every_emitted_site_has_a_successful_mapping() {
        let mut seen: Vec<Operator> = Vec::new();

        for source in TOTALITY_CORPUS {
            for site in scan(source) {
                let token = span_text(source, &site);
                assert!(
                    crate::operators::replacement(site.operator, token).is_ok(),
                    "{:?} emitted `{token}` in `{source}`, which has no mapping",
                    site.operator,
                );
                if !seen.contains(&site.operator) {
                    seen.push(site.operator);
                }
            }
        }

        for operator in Operator::ALL {
            assert!(
                seen.contains(&operator),
                "no corpus source emits {operator:?} — discovery for it is unpinned",
            );
        }
    }
}
