//! Committed TOML sidecar manifest (`<file>.rs.m4r.toml`) — read/write plus the
//! per-function hashing seam that drives differential runs.
//!
//! Unlike the pure-core [`crate::scanner`]/[`crate::site`] modules, this is an
//! **infrastructure/adapter** concern (design.md → the only layer touching fs):
//! reading and writing the sidecar on disk is allowed here. The manifest mirrors
//! mutate4go's differential model (decisions C1 / A7): a reviewable, diff-friendly
//! sidecar recording the last-run time plus a per-function content hash, so a
//! later slice can mutate only changed functions.
//!
//! Determinism is a hard requirement (the sidecar is committed and reviewed):
//! [`Manifest::functions`] is a [`BTreeMap`] so serialization has stable key order.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proc_macro2::Span;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::scanner::{Frame, scope_path, self_ty_name};

/// Current manifest schema version. Bump when the on-disk format or the
/// hash-semantics change incompatibly, so a mismatched sidecar is detectable
/// rather than silently mis-read.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Identifier of the interim hashing algorithm (std [`DefaultHasher`] over the
/// function's source slice). Recorded in the manifest so R7 toolchain churn or
/// the T14 normalized-hash swap is detectable across sidecars; the `// TODO(T14)`
/// swap will change this id.
pub const INTERIM_HASHER_ID: &str = "interim-defaulthasher-v0";

/// The committed sidecar manifest for a single source file.
///
/// `last_run` is **Unix epoch seconds** (dependency-free; compared against the
/// source file's mtime for `--since-last-run` in a later slice — deliberately no
/// date crate). `functions` maps each `function_id` to its content hash, using a
/// [`BTreeMap`] for deterministic, diff-friendly serialization.
///
/// `schema_version` and `hasher` are format markers serialized at the top of the
/// TOML (before the `[functions]` table). Both are `#[serde(default)]` so a
/// sidecar written without them — by an older or newer tool — still deserializes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version marker (see [`CURRENT_SCHEMA_VERSION`]).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Identifier of the hashing algorithm used for `functions` (see
    /// [`INTERIM_HASHER_ID`]).
    #[serde(default = "default_hasher")]
    pub hasher: String,
    /// Last run time, as whole seconds since the Unix epoch.
    pub last_run: u64,
    /// `function_id` → content hash. Never keyed by `byte_span`: byte offsets are
    /// ephemeral run-local data and must never be persisted as manifest identity.
    pub functions: BTreeMap<String, String>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            hasher: INTERIM_HASHER_ID.to_owned(),
            last_run: 0,
            functions: BTreeMap::new(),
        }
    }
}

/// serde default for [`Manifest::schema_version`] on backward-compatible reads.
fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// serde default for [`Manifest::hasher`] on backward-compatible reads.
fn default_hasher() -> String {
    INTERIM_HASHER_ID.to_owned()
}

/// Derives the sidecar path by **appending** `.m4r.toml` to the full source file
/// name — `src/foo.rs` → `src/foo.rs.m4r.toml`. The `.rs` extension is kept (not
/// replaced) so the sidecar sits visibly beside its source.
#[must_use]
pub fn sidecar_path(source: &Path) -> PathBuf {
    let mut name = source.file_name().unwrap_or_default().to_owned();
    name.push(".m4r.toml");
    source.with_file_name(name)
}

/// Reads a manifest from `path`.
///
/// Returns `Ok(None)` when the sidecar does not exist — absence is the normal
/// first-run case, not an error.
///
/// # Errors
///
/// Returns an error only on genuine I/O failure or malformed TOML.
pub fn read(path: &Path) -> Result<Option<Manifest>> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let manifest = toml::from_str(&text)
                .with_context(|| format!("malformed manifest `{}`", path.display()))?;
            Ok(Some(manifest))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => {
            Err(err).with_context(|| format!("failed to read manifest `{}`", path.display()))
        }
    }
}

/// Serializes `manifest` to deterministic TOML and writes it to `path`
/// (create/overwrite).
///
/// # Errors
///
/// Returns an error on serialization or I/O failure.
pub fn write(path: &Path, manifest: &Manifest) -> Result<()> {
    let text = toml::to_string(manifest)
        .with_context(|| format!("failed to serialize manifest `{}`", path.display()))?;
    std::fs::write(path, text)
        .with_context(|| format!("failed to write manifest `{}`", path.display()))
}

/// Enumerates the named functions in `source` and returns `function_id` → content
/// hash, deterministically ordered.
///
/// The keys use the **same derivation scheme as [`crate::scanner`]** (module /
/// impl / trait idents joined to `fn` idents by `::`, `self_ty_name` for impls,
/// nested `fn`s get their own id) via the shared [`scope_path`] helper, so
/// differential selection lines up 1:1 with scan sites.
///
/// # Errors
///
/// Returns an error if `source` is not valid Rust.
pub fn function_hashes(source: &str) -> Result<BTreeMap<String, String>> {
    let file =
        syn::parse_file(source).context("failed to parse Rust source for function hashing")?;
    let mut collector = FunctionCollector::new(source);
    collector.visit_file(&file);
    Ok(collector.hashes)
}

/// Immutable-visitor state for [`function_hashes`]: the shared scope stack plus
/// the accumulated per-function hashes.
///
// TODO(T14/T5): decide differential home for None-function (file-level) sites —
// module-level / associated `const` sites have no function home, so for now they
// are simply not represented in the manifest (enumeration is over named
// functions only).
struct FunctionCollector<'a> {
    source: &'a str,
    scope: Vec<Frame>,
    hashes: BTreeMap<String, String>,
}

impl<'a> FunctionCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            scope: Vec::new(),
            hashes: BTreeMap::new(),
        }
    }

    /// Records the current function's hash under its `function_id`. `span` covers
    /// the whole function item, so the hash is over the function's source slice.
    fn record(&mut self, span: Span) {
        let id = scope_path(&self.scope);
        let range = span.byte_range();
        let hash = hash_slice(&self.source[range]);
        self.hashes.insert(id, hash);
    }
}

impl<'ast> Visit<'ast> for FunctionCollector<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        self.record(node.span());
        visit::visit_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        self.record(node.span());
        visit::visit_impl_item_fn(self, node);
        self.scope.pop();
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.scope.push(Frame::Function(node.sig.ident.to_string()));
        self.record(node.span());
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
}

/// Interim per-function content hash: a `std` [`DefaultHasher`] over the
/// function's source-slice text, hex-encoded.
///
// TODO(T14): replace interim hash with normalized syn token-reprint hashing.
fn hash_slice(slice: &str) -> String {
    let mut hasher = DefaultHasher::new();
    slice.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{
        CURRENT_SCHEMA_VERSION, INTERIM_HASHER_ID, Manifest, function_hashes, read, sidecar_path,
        write,
    };
    use crate::scanner::scan_source;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn round_trips_through_disk() {
        let mut functions = BTreeMap::new();
        functions.insert("foo".to_owned(), "deadbeef".to_owned());
        functions.insert("S::bar".to_owned(), "cafef00d".to_owned());
        let manifest = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            hasher: INTERIM_HASHER_ID.to_owned(),
            last_run: 1_700_000_000,
            functions,
        };

        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("roundtrip.rs.m4r.toml");
        write(&path, &manifest).expect("write should succeed");
        let read_back = read(&path).expect("read should succeed");

        assert_eq!(read_back, Some(manifest.clone()));
        // The freshly-written manifest carries the current format markers.
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.hasher, INTERIM_HASHER_ID);
    }

    #[test]
    fn read_without_markers_defaults_to_current() {
        // A sidecar written by an older tool — only `last_run` + `[functions]`,
        // no `schema_version` / `hasher` — must still deserialize, defaulting the
        // markers to the current constants (serde `default`).
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("legacy.rs.m4r.toml");
        std::fs::write(&path, "last_run = 7\n\n[functions]\nfoo = \"deadbeef\"\n")
            .expect("seed legacy sidecar");

        let manifest = read(&path)
            .expect("legacy sidecar reads without error")
            .expect("legacy sidecar is present");

        assert_eq!(manifest.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(manifest.hasher, INTERIM_HASHER_ID);
        assert_eq!(manifest.last_run, 7);
        assert_eq!(
            manifest.functions.get("foo").map(String::as_str),
            Some("deadbeef")
        );
    }

    #[test]
    fn read_missing_sidecar_is_none() {
        // A path inside a fresh temp dir that is guaranteed absent (never written).
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("missing.rs.m4r.toml");
        assert_eq!(read(&path).expect("absence is not an error"), None);
    }

    #[test]
    fn read_malformed_toml_is_err() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("malformed.rs.m4r.toml");
        std::fs::write(&path, "this is not = valid = toml").expect("seed file");
        let result = read(&path);
        assert!(result.is_err(), "malformed TOML must be an error");
    }

    #[test]
    fn sidecar_path_appends_and_keeps_rs_extension() {
        assert_eq!(
            sidecar_path(Path::new("src/foo.rs")),
            PathBuf::from("src/foo.rs.m4r.toml"),
        );
    }

    #[test]
    fn serialization_is_deterministic() {
        // Same content, insertion in different orders → byte-identical TOML,
        // because `functions` is a BTreeMap (stable key ordering).
        let mut a = BTreeMap::new();
        a.insert("b".to_owned(), "2".to_owned());
        a.insert("a".to_owned(), "1".to_owned());
        let mut b = BTreeMap::new();
        b.insert("a".to_owned(), "1".to_owned());
        b.insert("b".to_owned(), "2".to_owned());

        let one = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            hasher: INTERIM_HASHER_ID.to_owned(),
            last_run: 42,
            functions: a,
        };
        let two = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            hasher: INTERIM_HASHER_ID.to_owned(),
            last_run: 42,
            functions: b,
        };

        assert_eq!(
            toml::to_string(&one).unwrap(),
            toml::to_string(&two).unwrap(),
        );
    }

    #[test]
    fn function_hash_keys_match_scanner_function_ids() {
        // A free fn, an impl method, and a nested fn — each with a `+` site so the
        // scanner attributes a `function_id` to it. The manifest keys must equal
        // the set of scanner function_ids for the same source.
        let source = concat!(
            "fn free_fn(a: i32, b: i32) -> i32 { a + b }\n",
            "struct S;\n",
            "impl S {\n",
            "    fn method(a: i32, b: i32) -> i32 {\n",
            "        fn nested(x: i32, y: i32) -> i32 { x + y }\n",
            "        nested(a, b) + a\n",
            "    }\n",
            "}\n",
        );

        let scanner_ids: std::collections::BTreeSet<String> = scan_source(source)
            .expect("fixture should parse")
            .into_iter()
            .filter_map(|s| s.function_id)
            .collect();
        let hash_keys: std::collections::BTreeSet<String> = function_hashes(source)
            .expect("fixture should parse")
            .into_keys()
            .collect();

        assert_eq!(hash_keys, scanner_ids);
        assert_eq!(
            hash_keys,
            ["S::method", "S::method::nested", "free_fn"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        );
    }
}
