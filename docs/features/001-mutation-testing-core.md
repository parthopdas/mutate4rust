# Feature: Mutation Testing Core (mutate4rust)
**Branch:** vibe/001-mutation-testing-core
**Status:** In Progress

## Requirements

Port `unclebob/mutate4go` to an idiomatic Rust CLI, `mutate4rust`, that operates **one source
file at a time** and:

- Discovers mutation sites in a `.rs` file, applies each mutation, runs the crate's tests, and
  reports **killed / survived / uncovered** mutants.
- Uses coverage to mutate only covered sites (uncovered sites are reported, not executed).
- Maintains a per-file manifest (last-run date + per-function hashes) to drive **differential**
  mutation ("changed functions only") on subsequent runs.
- Preserves mutate4go's **CLI/UX surface** (flags, subcommand-style modes) so users transfer
  muscle memory 1:1.
- Ships **Rust-native, idiomatic mutation operators active by default** — not merely the
  language-universal parity set.

Upstream reference (verified against `unclebob/mutate4go`): runs coverage automatically (unless
`--reuse-coverage`), runs a baseline test command, applies each covered mutation with a timeout and
restores the file, writes an embedded footer manifest, defaults to differential when that manifest
exists, and reports uncovered sites without spending test time on them. Classification is by test
exit code only (compile failure ⇒ non-zero exit ⇒ `killed`); it emits raw counts, no numeric score.

## Design Options (Ox)

### O1 — Faithful port + idiomatic operator extension (RECOMMENDED, human-selected)
- Description: Mirror mutate4go's pipeline (discover → coverage → mutate → test → report → manifest)
  and one-file-at-a-time model, implemented natively in Rust: `syn`/`proc-macro2` for parsing and
  spans, `cargo-llvm-cov` for coverage, `cargo test` as the test runner, a **committed TOML sidecar
  manifest**, and a mutation operator set that keeps mutate4go's mappings while adding idiomatic
  Rust operators **active by default**.
- Pros: Preserves upstream UX/workflow; leverages the Rust ecosystem's canonical crates; idiomatic
  operators give real Rust-relevant mutation coverage; sidecar manifest is reviewable and keeps
  source files clean.
- Cons: Per-mutation recompile+test cost is higher in Rust than Go; more operators than upstream ⇒
  not count-parity; llvm-cov region→line mapping adds fidelity work.

### O2 — Wrap an existing Rust mutation tester (e.g. `cargo-mutants`)
- Description: Shell over a mature Rust mutation tool and re-skin its output to mutate4go's CLI.
- Pros: Least implementation effort; battle-tested mutant engine.
- Cons: Loses the point of a faithful mutate4go port — its differential manifest model, single-file
  workflow, coverage-gated site selection, and flag surface don't map cleanly; we'd inherit an
  external tool's roadmap and semantics. Rejected.

### O3 — Minimal parity-only port (defer Rust-specific operators)
- Description: Port only the language-universal operator set, matching mutate4go's mutation **count**
  as closely as possible; Rust operators opt-in/later.
- Pros: Smallest first cut; strict count-parity is easy to test against upstream.
- Cons: Under-tests Rust code (misses `Option`/`Result`, `?`, `match`, bitwise, etc.); count-parity
  is a weak goal for a Rust tool. Explicitly rejected by the human — Rust operators are wanted
  native, idiomatic, and **on by default**.

**Recommended: O1 — faithful workflow/UX parity with idiomatic Rust operators active by default.
The human has selected O1.**

## Slices (Sx)

A slice is defined in `docs/meta-design.md`. Each is independently deployable and end-to-end
verifiable. **S1 is the greenfield bootstrap** — it creates the Cargo project so the profile's gate
commands run, and replaces the `docs/design.md` FILL_ME stub (preflight blocker) with the real
high-level design.

| Slice | Outcome | Depends on |
|-------|---------|------------|
| S1 | **Bootstrap**: Cargo binary crate `mutate4rust`; CLI skeleton parsing the full parity flag surface (wired to stubs); `--help`/`--version`; all profile gates green (`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test`); **replace `docs/design.md` stub with real high-level design**. E2E: `mutate4rust --help` lists flags; gates pass. | - |
| S2 | Discovery & scan: `syn`-based mutation-site model, `--scan` (site + changed-site counts + mutation-warning), sidecar manifest read/write scaffold, `--update-manifest`. | S1 |
| S3 | Core mutate loop + universal & arithmetic-**parity** operators: byte-span apply/restore, `cargo test` runner with timeout, killed/survived/uncovered report. Operators = the mutate4go set. | S2 |
| S4 | Arithmetic-family **idiomatic completion**: `/→*`, `%→*`, and compound-assignment operators. | S3 |
| S5 | Coverage integration: `cargo-llvm-cov`, covered-only execution, uncovered reporting/skip, `--reuse-coverage`, coverage-absent parity behavior. | S3 |
| S6 | **Rust-specific operators** (idiomatic, active by default): `Option`/`Result`, `match`-arm, `unwrap`/`expect`, `?`, bitwise. | S3 |
| S7 | Differential mutation & manifest lifecycle: per-function normalized hashing, default-differential-when-manifest-exists, `--since-last-run`, `--mutate-all`, `--lines`, post-run manifest update. | S2, S3 |
| S8 | Parallelism & remaining CLI parity: `--max-workers` (isolated worker dirs), `--mutation-warning`, `--timeout-factor`, `--test-command`, `--verbose`. | S3 |

Sequencing rationale (per human decision #2): parity operators land first (S3), arithmetic idiomatic
completions next (S4), and the Rust-specific operators are their **own follow-on slice** (S6),
decoupled from coverage (S5) so either can proceed once S3 is in.

## Tasks (Tx)

One or more tasks per slice.

| #  | Slice | Task | Status | Commit |
|----|-------|------|--------|--------|
| T1  | S1 | `cargo init` binary crate `mutate4rust`; deps (`clap`, `syn` w/ full+span features, `proc-macro2`, `anyhow`, `toml`, `serde`); trivial lib fn + unit test so gates pass. | Done | ✅ |
| T2  | S1 | clap CLI: positional `<FILE>` + all parity flags (parse only, wired to stubs); `--help`/`--version`; snapshot test; exit-code contract. | Done | ✅ |
| T3  | S1 | Replace `docs/design.md` FILL_ME stub with real high-level design (overview, layers, components, cross-cutting, conventions). | Done | ✅ |
| T4  | S2 | `syn`/`proc-macro2` parse + mutation-site model (kind, byte span, line, function id). | Done | ✅ |
| T5  | S2 | Sidecar manifest (`<file>.rs.m4r.toml`) schema + read/write; `--update-manifest`. | Done | ✅ |
| T6  | S2 | `--scan` mode: total sites, changed sites vs manifest (stub 0 until S7), mutation-count warning (default 50). | Done | ✅ |
| T7  | S3 | Byte-span mutant apply + guaranteed restore (in-memory original; restore even on panic). | Done | ✅ |
| T8  | S3 | `cargo test` runner with per-mutant timeout; classify killed/survived/uncovered (timeout and non-compiling folded into killed, per Go parity). | Done ✅ | cea810a |
| T9  | S3 | Universal + arithmetic-parity operators (see taxonomy) + result reporter (Killed/Survived/Uncovered). | Done ✅ | 54d30e2 |
| T10 | S4 | Arithmetic idiomatic completions: `/→*`, `%→*`, compound-assignment ops. | Done ✅ | 04b15fd |
| T11 | S5 | `cargo-llvm-cov` invocation + profile parse; region→line coverage map; **add `llvm-tools-preview` + `cargo-llvm-cov` to both CI legs**; **verify A6 `--reuse-coverage`-without-coverage against upstream**. | Done | `a89ae64` |
| T12 | S5 | Covered-only gating; uncovered sites reported & skipped; `--reuse-coverage`; coverage-absent behavior (A6); **report gains per-mutant records (counters become derived)**. | Done | `2cf768c` |
| T13a | S6 | Token-level Rust operators that fit the current `Site` model: **float constants (`0.0↔1.0`, suffix preserved)**, bitwise (`&`↔`|`, `^→&`, `<<`↔`>>`), predicate-method swaps (`.is_some()`↔`.is_none()`, `.is_ok()`↔`.is_err()`). Plus T12 housekeeping: score-line qualifier, `#[cfg(test)]` gated in all **four** attribute-bearing item enums (`Item`, `ForeignItem`, `TraitItem`, `ImplItem`), `parse_export` into `mod tests`, target pre-flight parse. Also fixes a latent defect: `0f64` parses as a suffixed `LitInt` and was emitted as an integer site whose mapping then aborted the run. | Done | `3498087` |
| T13b | S6 | (1) extract `mutant_source(original, site)` (owed from T12); (2) `KillReason::CompileError` **before** any undecidable-precondition operator ships; (3) scanner↔mapping **totality property test**; (4) **no sites inside `syn::Type`** rule; (5) `Stmt`/`Arm` cfg gate sweep; (6) structural operators that fit the current model (`Some(x)→None`, `Ok(x)→Err(_)`, `expr?→expr.unwrap()`, `.unwrap()`/`.expect(_)→.unwrap_or_default()`, match-arm **guard drop**) + record rendering generalizes `canonical_token` → mutation description; (7) bump `Operator::ALL.len()` deliberately. **No Core model change.** | Pending | - |
| T13c | S6 | `Vec<Edit>` + `splice_all` (descending start order) driven by **arm-body swap alone** — the only S6 operator that cannot be one edit. `replacement` survives as the single-token fast path. Blast radius: `site.rs`, `apply.rs` (+1 fn), one block in `pipeline.rs`. | Pending | - |
| T14 | S7 | Per-function normalized hashing (deterministic `syn` token reprint); differential selection; default-differential-when-manifest-exists. | Pending | - |
| T15 | S7 | `--since-last-run`, `--mutate-all`, `--lines`; post-run manifest update; wire changed-count into `--scan`. | Pending | - |
| T16 | S8 | `--max-workers` isolated worker dirs (isolated target/source copy, seeded from warmed baseline) + aggregation. | Pending | - |
| T17 | S8 | Test-phase timing + `--timeout-factor`; `--mutation-warning`, `--test-command`, `--verbose`. | Pending | - |

## Operator Taxonomy

Two-source model: **parity mappings** are byte-identical to mutate4go (so the same site mutates the
same way); **idiomatic** entries are Rust-native additions. All are **active by default**.

### Universal / parity operators (S3)
| Class | Mapping | Source |
|-------|---------|--------|
| Arithmetic | `a + b → a - b`; `a - b → a + b`; `a * b → a / b` | parity |
| Comparison | `>` ↔ `>=`; `<` ↔ `<=` | parity |
| Equality | `==` ↔ `!=` | parity |
| Boolean literal | `true` ↔ `false` | parity |
| Logical | `&&` ↔ `||` | parity |
| Constant | `0` ↔ `1` | parity |

### Arithmetic idiomatic completions (S4)
| Class | Mapping | Source |
|-------|---------|--------|
| Arithmetic (division) | `a / b → a * b` | idiomatic |
| Arithmetic (remainder) | `a % b → a * b` | idiomatic |
| Compound assignment | `+= ↔ -=`; `*= → /=`; `/= → *=`; `%= → *=` | idiomatic |

> **Arithmetic-parity reconciliation (unambiguous):** mutate4go mutates only `+→-`, `-→+`, `*→/`
> (no `/→*`, no `%`, no assignment-ops). We keep those three parity mappings **identical** and only
> **add** the missing operators idiomatically — we never alter an existing parity mapping. A file run
> through both tools therefore produces the same mutations for `+`, `-`, `*`, plus additional
> mutations for `/`, `%`, and compound-assignments. This is the direct cause of the not-count-parity
> tradeoff in A5.

### Rust-specific operators (S6, idiomatic, active by default)
| Class | Mapping (intent) | Source |
|-------|------------------|--------|
| `Option` | `Some(x) → None`; `.is_some()` ↔ `.is_none()` | idiomatic |
| `Result` | `.is_ok()` ↔ `.is_err()`; `Ok(x) → Err(_)` where a bound error value exists | idiomatic |
| `match`-arm | drop an arm guard (`if guard` removed); swap two non-wildcard arm bodies | idiomatic |
| `unwrap`/`expect` | `.unwrap()` / `.expect(_)` → `.unwrap_or_default()` (where `T: Default`) | idiomatic |
| Float constant | `0.0` ↔ `1.0` (suffix `f32`/`f64` preserved) | idiomatic |
| `?` operator | `expr? → expr.unwrap()` where the `Try` type permits | idiomatic |
| Bitwise | `&` ↔ `|`; `^ → &`; `<<` ↔ `>>` | idiomatic |

> Where a mapping's precondition doesn't hold (e.g. no `Default`, incompatible `Try` type), the site
> is **not** emitted rather than emitting a knowingly non-compiling mutant. Mutants that still fail to
> compile at test time are classified per A8.

## Risks (Rx)

- R1: **Rust per-mutant recompile+test cost** dominates runtime on large files. Mitigations:
  covered-only gating (S5), differential runs (S7), parallel workers (S8); deeper incremental-build
  caching is deferred (D2).
- R2: **Span-faithful rewriting.** Mutants must be applied as byte-span text splices over the
  original source (not `syn` reprint) to avoid reformatting the whole file and corrupting spans/diffs.
- R3: **Rust arithmetic panics.** Debug builds panic on overflow, `*→/` can divide-by-zero — may
  trivially kill arithmetic mutants and diverge debug vs release. Accept parity default (debug
  `cargo test`); release/overflow toggle deferred (D6).
- R4: **Non-compiling mutants** waste a compile. Per Go parity they are counted as **killed** (a
  broken build fails the test command, non-zero exit), matching mutate4go exactly (A8). Precondition
  gating (taxonomy note) avoids emitting knowingly-broken mutants where feasible.
- R5: **llvm-cov region→line fidelity.** Region coverage mapped to lines may mis-mark sites as
  uncovered; validate mapping against known-covered fixtures (S5).
- R6: **Worker isolation.** Parallel workers need isolated `target/`/source copies (seeded from a
  warmed baseline) to avoid cargo lock contention and cross-mutant interference (S8).
- R7: **Manifest hash stability** across `syn`/toolchain versions; normalization must be deterministic
  (S7) or differential runs churn.
- R8: **Coverage-tool dependency** (`cargo-llvm-cov` + `llvm-tools-preview`) must be present in dev +
  CI (verify the Windows CI leg); absence must fail clearly, not silently mutate everything.
- R9: **`syn` limits** — macro-generated / `cfg`-excluded code isn't represented → missed/false sites.

## Assumptions (Ax)

- A1: `syn` (full + span features) parses target files (incl. common macro usage) sufficiently for
  site discovery; files that don't parse fail fast with a clear error.
- A2: `cargo-llvm-cov` is the coverage backend; profiles/artifacts live under `target/` (llvm-cov's
  convention), cross-platform incl. the Windows CI leg.
- A3: **One-file-at-a-time** model matches upstream; whole-crate/multi-file batch mutation is out of
  scope for MVP (D1).
- A4: Per-mutant recompile+test cost is acceptable for MVP; performance optimization beyond
  covered-only/differential/parallel is deferred (D2).
- A5: **Default runs are NOT count-parity with mutate4go.** Idiomatic Rust operators are active by
  default and produce more mutation sites than upstream. Parity is retained **only** for (a) the
  CLI/UX surface and (b) the arithmetic-family parity *mappings* (`+→-`, `-→+`, `*→/`). This is an
  **accepted tradeoff**: a Rust mutation tool should test Rust idioms, and count-parity with a Go
  tool is not a meaningful goal.
- A6: **Coverage-absent behavior, pinned to verified upstream:** mutate4go regenerates coverage
  automatically (running the test command with a coverage profile) **unless `--reuse-coverage`**;
  sites not in the profile are reported uncovered and skipped (not mutated, not errored). We mirror
  this: by default run a `cargo-llvm-cov` pass first, then mutate only covered sites and list
  uncovered ones. **Unverified edge:** `--reuse-coverage` set but no coverage file present — flagged
  for verification during S5 (provisional: error asking the user to run coverage or drop
  `--reuse-coverage`). Do not guess silently.
- A7: Manifest is a **committed TOML sidecar** `<file>.rs.m4r.toml` next to the source (decision #1),
  diverging from upstream's embedded footer manifest.
- A8: **Non-compiling mutants** are counted as **`killed`**, in full parity with mutate4go: a broken
  build fails the test command (non-zero exit) and upstream classifies that as killed. **No separate
  `invalid` bucket.** Report buckets = **Killed / Survived / Uncovered** (timeout also folded into
  killed, per upstream); score = `killed / (killed + survived)`, with `uncovered` excluded and
  reported separately. *(Verified against `unclebob/mutate4go` `internal/runner/runner.go`
  `runMutant`/`summarize`.)* Precondition-failing operator sites are still not emitted (taxonomy
  note) to avoid wasting compiles on knowingly-broken mutants — but any that slip through and fail to
  compile are scored as killed.

## Deferrals (Dx)

- D1: Whole-crate / multi-file / directory batch runs (MVP is single-file, per upstream).
- D2: Incremental/cached-compilation performance optimization beyond covered-only + differential +
  parallel workers.
- D3: User-configurable operator enable/disable (config file or `--operators` selection). All
  operators ship active by default; selection is a later enhancement.
- D4: HTML / structured (JSON) reports and editor/IDE integration; MVP output is CLI text.
- D5: `cargo mutate4rust` cargo-subcommand packaging (MVP ships standalone `mutate4rust <FILE>`).
- D6: Release-mode / overflow-config toggle for arithmetic mutants.
- D7: Mutation of macro-internal / generated code and `build.rs`.
- D8: `prettyplease` re-emit fallback for patching.

## Notes & Decisions

### Human sign-off
1. **C1 manifest store = committed sidecar** `<file>.rs.m4r.toml` (keeps `.rs` fmt-clean; cross-clone
   differential parity without upstream's in-source footer).
2. **C5 = do NOT defer Rust operators** — native, idiomatic, active by default (S6); A5 accepts
   non-count-parity as an intentional tradeoff.
3. **Coverage-absent = mutate4go parity**, pinned to verified upstream behavior (A6); the
   `--reuse-coverage`-without-coverage edge is flagged for S5 verification, not guessed.
4. **Feature = `001-mutation-testing-core`**, branch `vibe/001-mutation-testing-core`.
5. **A8 non-compiling mutants = counted as `killed`** (full mutate4go parity; no separate `invalid`
   bucket).

### Report shape
Three buckets — **Killed / Survived / Uncovered** (timeout and non-compiling folded into Killed,
exactly as mutate4go). Score = `killed / (killed + survived)`; `uncovered` is excluded from the score
and reported separately. Full parity with mutate4go's bucket assignment.

### Conflict-resolution summary (C1–C10 outcomes)
| # | Conflict (mutate4go/Go assumption) | Resolution for mutate4rust |
|---|-------------------------------------|----------------------------|
| C1 | Manifest embedded as a source-file footer | **Committed TOML sidecar** `<file>.rs.m4r.toml` — reviewable, keeps `.rs` clean, shared across dev/CI. |
| C2 | Coverage via `go test -coverprofile` (built-in) | **`cargo-llvm-cov`** (stable Rust has no built-in line coverage); `--reuse-coverage` retained. |
| C3 | Parsing via `go/ast` | **`syn` + `proc-macro2`** for AST + byte spans. |
| C4 | Test runner `go test ./...` | **`cargo test`**; overridable via `--test-command`. |
| C5 | Language-universal operators only | **Rust-native idiomatic operators added, active by default** (S6). |
| C6 | In-place source rewrite | **Byte-span text splice** over original bytes; restore from in-memory copy (never `syn`-reprint the file). |
| C7 | Timeout = factor × baseline test time | **`--timeout-factor` on measured test phase**, separated from compile time. |
| C8 | Line-granular Go coverage profile | **llvm-cov region coverage mapped to lines** for parity in reporting/gating. |
| C9 | Parallel workers copy the Go module | **Isolated per-worker target/source copies seeded from a warmed baseline**; `--max-workers`. |
| C10 | mutate4go flag/mode surface | **1:1 CLI parity** (`--scan`, `--update-manifest`, `--lines`, `--since-last-run`, `--mutate-all`, `--reuse-coverage`, `--mutation-warning`, `--timeout-factor`, `--test-command`, `--max-workers`, `--verbose`, `--help`). |

### Testing guidance (per golden rule #8)
- **Unit:** operator mapping correctness (each taxonomy row → expected mutated bytes); site discovery
  on fixtures; manifest read/write + normalized hashing stability; coverage region→line mapping;
  differential selection (changed vs unchanged functions); mutant apply/restore round-trip;
  classification (killed incl. timeout & non-compiling / survived / uncovered) incl. the score formula.
- **Integration (critical paths only):** end-to-end single-file run (discover → coverage → mutate →
  test → report) on a tiny fixture crate with a known killed/survived/uncovered outcome;
  `--scan` counts; `--reuse-coverage` and coverage-absent behavior (A6); one `--max-workers > 1` run
  for worker isolation. Avoid timing-sensitive assertions on the test-timeout path.

### Open item flagged for later verification
- A6 `--reuse-coverage`-without-coverage edge — resolve against upstream source during S5.

### Task reviews — Anders (design)
- **T1 — APPROVE-WITH-SUGGESTIONS** (no blockers). Guidance to carry forward:
  - **T3 must-do:** write the **inward dependency-flow / pure-core boundary** into `docs/design.md`:
    *Core/domain* (pure: mutation-site model + operator mappings — no fs/process/clap) ← *Application*
    (discover→coverage→mutate→test→report pipeline) ← *Infrastructure/adapters* (`cargo test` runner,
    `cargo-llvm-cov`, TOML sidecar I/O, clap CLI). Dependency arrows point inward; `operators`/site
    model must never `use` runner/coverage/fs. No premature trait seams (YAGNI) — just name the
    boundary so downstream tasks land on the right side.
  - Modules live **inside the lib**; `main.rs` stays a thin outer adapter. No workspace/multi-crate
    split at MVP.
  - **Caveat for T4/T5:** a green T1 build proves dep versions *resolve*, not that `syn`/`serde`/`toml`
    feature flags are correct — those aren't compiled-against until first use. T4/T5 should explicitly
    confirm the enabled feature sets.
  - Non-blocking niceties: consider `rust-version = "1.85"` (MSRV) in `[package]`;
    `version_matches_cargo_manifest` test is tautological scaffolding (harmless, will be superseded).
- **T2 — APPROVE-WITH-SUGGESTIONS** (no blockers; commit not gated). Verified against upstream
  `unclebob/mutate4go` source.
  - **⚠ PRODUCT DECISION (pin before S3/T8) — exit-code contract is a deliberate divergence, NOT
    parity.** Upstream exits **0 even when mutants survive** and collapses every error (usage or
    operational) to **1** (`runner.StatusCode` returns 1 on any `err`; `Run` returns nil on
    survivors). Our proposed `0=success / 1=survivors / 2=usage / 3=error` is the better *guardrail*
    design (fail CI on survivors) but breaks CI parity for existing mutate4go pipelines. See "Product
    decision — exit codes (C11)" below. Only `Success`(0) is emitted until S3, so it doesn't gate T2.
  - **Verified parity ✓:** flag names + `--` spelling, `--verbose` as bool, `--lines` = comma-sep
    positive ints (help text accurate), required `<FILE>`, `--mutation-warning` default 50.
  - **T17:** upstream `--timeout-factor` is `int` default **10**; ours is `f64` with no default —
    apply default 10 when wired.
  - **S2/S7/S8:** add upstream's mutual-exclusivity rules (`--scan` vs update/exec opts;
    `--since-last-run`/`--mutate-all`/`--lines` mutually exclusive) via clap `conflicts_with`/
    `ArgGroup` (→ exit 2). None exist yet (parse-only).
  - **T3 design.md must document:** adapter `Cli` → validated **clap-free `RunConfig`** handoff
    (mirror upstream `cli.Options`; parse `lines: Option<String>` → `BTreeSet<usize>`, apply defaults
    at the boundary); model scan/update-manifest/mutate as a **`Mode` enum** resolved at the boundary
    (note latent gap: `run()` currently routes `--update-manifest` into the mutation stub).
  - Optional: `--reuse-lcov` alias for strict parity with upstream's synonym.
- **T3 — APPROVE-WITH-SUGGESTIONS** (no blockers; commit not gated). `docs/design.md` is correct,
  crisp, usable as the reloaded SSOT; inward dependency-flow rule, CLI→`RunConfig`→pipeline seam,
  `Mode` enum, key components, and the C11 exit-code parity contract all documented faithfully. Three
  doc-only polish items were applied before commit: (1) pipeline arrow now includes the trailing
  `→ manifest` stage; (2) numeric score clarified as an **additive** mutate4rust extension (buckets
  are 1:1 parity, score is not — upstream emits raw counts); (3) forward-pointer that future CLI
  mutual-exclusivity violations also resolve to exit `1` under C11. C11 exit-code correction in
  `src/cli.rs` verified faithful (0=success-incl-survivors / 1=any-error; clap usage `2`→`1`).
- **T4 — APPROVE-WITH-SUGGESTIONS** (no blockers). Site model (`src/site.rs`) + `scan_source`
  (`src/scanner.rs`) are architecturally sound, correctly scoped to the T4 in-scope operator set, and
  land on the right side of the pure-core boundary (both modules `use` only `syn`/`proc-macro2`/
  `anyhow` — no fs/process/argv/clap; `scan_source(&str)` takes source text, not a path). Immutable
  `Visit` + operator-token `byte_span` splice strategy confirmed correct (never `syn`-reprinted; C6/R2
  posture). `function_id` is name-based (invariant under reordering/insertion) — the right identity
  choice. T4/T5 feature-flag caveat now satisfied (`visit`, `spanned`, `byte_range()` all
  compile-exercised). Carry-forward notes:
  - **T14 (must-fix before differential):** `function_id` is **not unique** — `self_ty_name` keys impl
    blocks on the self-type's last path segment only and drops the trait, so `impl Display for S::fmt`
    and `impl Debug for S::fmt` collide, as do inherent vs. trait `S::f`, `Wrapper<i32>` vs.
    `Wrapper<u8>`, and non-path self-types (`_::method`). Two functions sharing one id collide in the
    per-function manifest map → missed mutations / differential churn. Fix is localized to
    `visit_item_impl`/`self_ty_name` (fold the trait into the qualifier, e.g. `<S as Display>::fmt`).
    Does not invalidate the T4 model.
  - **T5/T14:** define a differential strategy for `function_id == None` sites (module-level /
    associated `const` have no manifest home — e.g. synthetic file-level bucket); document explicitly
    that `byte_span` is **ephemeral run-local** and must **never** be persisted as manifest identity
    (valid only against the exact in-memory snapshot it was scanned from).
  - **T9:** constant mutation must **preserve literal suffix/radix** — constant sites carry the whole
    literal token span (`1u8`, `0x1`), so a naive splice of `"0"` would drop the `u8` suffix and break
    type inference/compile. Preserve suffix/radix (or narrow the span) in the constant mapping.
  - **T13/S6:** the single-`byte_span`/single-`Operator` `Site` won't fit multi-span/structural
    operators (arm-body swap needs two spans; `Some(x)→None`, `expr?→expr.unwrap()` replace a subtree).
    Generalize the model then (YAGNI until T13) — don't assume single-token splice is the whole
    contract.
- **T5 — APPROVE-WITH-SUGGESTIONS** (no blockers). Manifest (`src/manifest.rs`, infra adapter) +
  `Mode` wiring are well-shaped: behavior-preserving scanner refactor, fs I/O correctly in the infra
  layer, deterministic `BTreeMap`/byte-identical TOML, and it **closes the design.md latent gap**
  (`--update-manifest` now routes to a real `Mode::UpdateManifest`, not the mutate stub). Carry-forward
  (c) holds — `byte_span` is never persisted (key = `function_id`, value = hash). One hedge was landed
  in T5 before commit (below); the rest are T14/T15 decisions owed to the human.
  - **LANDED IN T5 (Anders' item 1):** added a **schema/version marker** to the manifest —
    `schema_version: u32` (`CURRENT_SCHEMA_VERSION = 1`) + `hasher: String`
    (`INTERIM_HASHER_ID = "interim-defaulthasher-v0"`), both `#[serde(default)]` backward-compatible,
    serialized top-of-file before `[functions]`. Rationale: the interim `DefaultHasher` is **not stable
    across Rust releases (R7)** and the T14 normalized-hash swap changes every stored hash with no
    structural change — the marker lets a consumer detect incompatibility (treat as full-run) instead
    of silently mis-reading. Landed now because **T6 is the first consumer** of these hashes.
  - **T14 MUST-FIX — `function_id` uniqueness is now a CORRECTNESS bug (promoted from cosmetic).** In
    the manifest, colliding ids mean `BTreeMap::insert` **overwrites** — one function's hash silently
    wins, so a change to the other is invisible to differential selection. Fix `self_ty_name` to
    include the trait ident + disambiguate overlapping impls before differential goes live. NOTE: the
    `function_hash_keys_match_scanner_function_ids` test does **not** guard this (scanner+manifest
    agree *because* they collide identically).
  - **T14 hash-swap granularity:** the seam swap is NOT just replacing `hash_slice` — `record` keeps
    only the span/string; normalized token-reprint needs the syn **node/tokens**. Expect to retype
    `record`/`hash_slice` to take `ToTokens` and touch all three `visit_*_fn` methods (all
    `pub(crate)`/private — no contract break).
  - **T14 invariant nuance:** the true relationship is `manifest_keys ⊇ scanner_function_ids` (manifest
    hashes **every** named fn; scanner emits ids only for fns containing a site). Equality holds only on
    curated fixtures. T14 differential must rely on the **superset** relation — a siteless `fn empty(){}`
    is hashed but has no scanner id and is NOT an invariant violation.
  - **T14/T15 — file-level (`None`-function) sites owe an explicit decision:** module-level /
    associated-`const` sites have no manifest home; differential will treat them as always-mutated or
    never-mutated by default. Decide explicitly (always-in vs. synthetic file-level hash bucket) — a
    human/product-adjacent design call, not a silent default.
  - **T14 (nice-to-have first):** move shared id-derivation (`Frame`/`scope_path`/`self_ty_name`) out of
    `scanner` (a Core module now imported *back* by infra `manifest`) into a dedicated pure module
    (e.g. `scope`/`function_id`) so the shared identity scheme has one obviously-shared home and the
    uniqueness fix lands in one place.
  - **T6:** `--scan` changed-count must read via `manifest::read` and tolerate `Ok(None)` (first run)
    as "all changed / stub 0 per plan" — absence is NOT an error (C11). Rely on `manifest_keys ⊇
    scanner_ids`, not equality.
  - **T15:** confirm mtime-vs-`last_run` direction against upstream (source mtime > `last_run` ⇒
    changed); `last_run` bumps on every `--update-manifest` even with no change → committed-sidecar
    churn (upstream parity — make it a conscious call). Land clap `ArgGroup` mutual-exclusivity
    resolving conflicts to **exit 1 BEFORE any manifest write** (so a mis-flagged invocation never
    mutates a sidecar then errors).
- **T6 — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S2 CLOSED — clear to proceed to S3.** `--scan`
  is cleanly seamed: pure `ScanReport { file, total_sites, changed_sites, warn_threshold }` (value +
  `summary()`/`warning()`/`exceeds_threshold()`, strict `>`) with the handler doing only I/O + stream
  routing (summary → stdout, advisory → stderr). The pure seam is **param-based, not adapter-based**
  (`scan_report()` extracts `&source` and hands `&str` inward, never threads `&Cli`) — exactly the
  inward-dependency posture; preserve it into S3. Absent manifest tolerated (`Ok(None)` → Success,
  changed = 0), read-only (no runner/mutation path reachable from `Mode::Scan`), C11 intact. Changed =
  hard `0` is acceptable **only because** the inline disclaimer makes the misread impossible — keep the
  disclaimer mandatory.
  - **S2 acceptance — all met** across T4 (syn site model) + T5 (sidecar read/write + `--update-manifest`,
    closes latent `Mode` gap) + T6 (`--scan` counts + mutation-warning). Coherence note (not a gap):
    `Mode::resolve` is **precedence-only** (`--scan --update-manifest` silently → scan, no error); the
    `ArgGroup`→exit-1 enforcement is a **recorded** deferral to T15/S8 — the silent-precedence→exit-1
    shift is a latent behavior change landing then (fine pre-1.0).
  - **S3 GUARDRAIL (carry-forward):** S3 seams (T7 apply/restore, T8 runner+classify, T9
    operators+reporter) must take **domain primitives** (`source: &str`, `timeout: Duration`,
    `&[Site]`), **never `&Cli`**. The clap-free `RunConfig` translation is **NOT** needed for S3 —
    introducing it now is YAGNI; the pressure that justifies it is S5/S7/S8 flag-wiring (`--lines`,
    `--timeout-factor`, `--test-command`, `--reuse-coverage`). Keep the T5/T6 primitive-extraction
    pattern so `RunConfig` can be introduced lazily rather than retrofitted out of `&Cli` coupling.
  - **T15 (carry-forward):** replace the **entire** `Changed sites:` line — value **and** the stale
    "differential not yet active" prose — not just the `STUB_CHANGED_SITES` constant, or the
    interpolated value will self-contradict the fixed disclaimer.
  - **T6 confirms owed (non-blocking):** verify warning→stderr routing matches upstream mutate4go (if
    upstream prints to stdout, note the defensible divergence); declare `--scan` **text** output
    human-oriented and **not** a stability contract pre-1.0 (structured/JSON scan output is D4) — the
    `scan_report_summary_is_stable_and_greppable` snapshot is a change-detector, not a frozen contract.
- **T7 — APPROVE-WITH-SUGGESTIONS** (no code changes). `src/apply.rs` realizes **C6** cleanly: pure
  `splice` (prefix+replacement+suffix, validates bounds + UTF-8 char boundaries → `Err`, never panics,
  never `syn`-reprints) + infra `RestoreGuard` (owns in-memory original; `write_mutant`/`restore`
  idempotent; `Drop` best-effort restore, never panics). Correctly layered (splice pure, guard the only
  fs touch), seams take domain primitives (no `&Cli`). All suggestions are carry-forwards, not defects.
  - **T8 loop pattern (prescribed):** `RestoreGuard::new(path)` ONCE → per-site
    `splice(guard.original(), &site.byte_span, repl)` (always from the pristine in-memory original →
    **zero mutation accumulation** between sites) → `write_mutant` → run tests/classify → `restore()`
    every iteration (idempotent, cheap; matters for terminal state + skip/early-break/error paths).
    Keep `splice`/guard **unfused** (no `apply_site()` convenience — purity is what makes splice
    unit-testable). Take domain primitives (`&str`, `Duration`, `&[Site]`, `&Path`), not `&Cli`.
  - **T8 durability note (document, don't over-engineer):** the original is **in-memory only** — on
    `SIGKILL`/OOM/power-loss with a mutant on disk, `Drop` never runs and the mutated bytes are the
    final on-disk state (and `fs::write` is non-atomic: crash mid-write can truncate). Backstop is
    **VCS** (`git checkout -- <file>`). T8 should state the "target under version control" assumption
    explicitly and, if cheap, note/refuse on a dirty target so crash-recovery is unambiguous.
  - **Repo-wide guard:** the `Drop` panic-restore relies on `panic = "unwind"` (default; confirmed no
    `[profile]` override). A future `panic = "abort"` silently defeats the safety-net — guard against
    that profile change (comment / CI check).
  - **T16 (composes cleanly — a strength):** `RestoreGuard` is **per-path** with no shared/global
    state, so N workers ⇒ N guards ⇒ N distinct isolated-copy paths ⇒ no cross-worker interference.
    Carry-forward: in multi-worker mode the guarded path MUST be the worker's **isolated copy**, never
    the canonical source — the pipeline (caller) owns that path decision, not the guard.
  - **Splice honesty caveat:** the char-boundary check guarantees **byte-safety** (no corruption/panic),
    NOT semantic correctness. A stale/off-by-a-token span from a mismatched source revision splices
    cleanly into garbage without error → becomes a non-compiling mutant → A8 folds into **Killed** (not
    a crash). Scanner (T4, proven byte-accurate) owns "the span means the operator"; T8 must not read
    the boundary check as a correctness guarantee.
  - **Doc debt (fold into T8):** `src/lib.rs` module-doc "Coverage, mutation, and reporting land in
    later slices" is now stale for **mutation** (apply landed). Fix opportunistically inside T8's work
    (T8 touches this area) — do not spin a standalone churn commit.
- **T8 — APPROVE-WITH-SUGGESTIONS** (no blockers; commit not gated). `src/runner.rs` (new, infra) is
  correctly layered: touches only `std::process`/`std::path`/`std::time`/`anyhow`/`wait_timeout`; no leak
  into pure Core (confirmed both directions — runner doesn't import site/scanner, and site/scanner import
  no process/fs/runner). Seams take domain primitives (`TestRunner::new(&[String], Duration, &Path)`,
  `classify(bool,bool)`), NOT `&Cli` — `RunConfig` correctly still not introduced (YAGNI). Pure `classify`
  (`#[must_use]`, total over 2 bools, unit-tested) split cleanly from side-effecting `TestRunner`/
  `run_and_classify`, with `RawRunOutcome{tests_passed,timed_out}` as intermediate seam. A8/Go parity
  faithful: passed→Survived, failed/compile-error/timeout→Killed, Uncovered reserved (never produced in
  T8, lands S5). Doc debt from T7 cleared. Gate green (58 tests). Suggestions + carry-forwards:
  - **Suggestion (non-blocking):** `classify(bool,bool)` takes two positional bools of the same type →
    easy call-site transposition. Optional: `classify(RawRunOutcome)` / `RawRunOutcome::classify(self)`
    for one source of truth. Current form keeps unit tests maximally direct — judgment call.
  - **T9 (reporter) — relocate `MutantOutcome`:** it's a domain type currently in infra `runner`; the
    reporter (Application) will `use crate::runner::MutantOutcome` (mild inward-flow smell). When the
    reporter lands, move `MutantOutcome` to a pure module (own `outcome` module or alongside site model)
    so the score buckets have a Core home and both reporter+runner depend inward on it.
  - **T16/T17 (process-tree kill — real resource concern, not cosmetic):** on timeout `child.kill()`
    reaps only `cargo`, not the grandchild test binary. Classification stays correct (→Killed), but an
    orphaned hung test process can linger holding the shared `target/` build lock and **stall/block the
    next mutant's `cargo test`** in single-worker mode. T16 isolated worker dirs don't fix the
    single-worker case. **T17 must implement process-group (Unix) / job-object (Windows) kill** so the
    whole subtree dies. Owed, not deferred-and-forgotten.
  - **T17 (fixed `Duration` false-kills):** T8 takes a caller-supplied fixed timeout → systematically
    false-kills slow-but-correct suites (inflates kill count, R3-adjacent). T17 must derive timeout from
    a measured baseline test-phase × `--timeout-factor` (default 10) to keep false-timeouts rare.
  - **T9 (pipeline wiring — two distinct paths + baseline):** loop must supply runner's `working_dir` as
    the **crate root** (so `cargo test` recompiles the mutated file) while `RestoreGuard::new` binds to
    the **target `.rs` file** — two separate caller-owned paths. T8 does **no baseline green-check**; the
    pipeline (T9) owns confirming the suite is green *before* mutating, else every mutant misclassifies.
    Keep `splice`/guard/runner **unfused** (no `apply_and_run`) — purity keeps splice+classify testable.
    In multi-worker mode (T16) both the guarded path and `working_dir` must be the worker's **isolated
    copy**, never canonical source.
- **T9 — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S3 CLOSED — clear to proceed to S4/T10.** All
  five T8 carry-forwards landed (suffix/radix preservation *plus* value-validation rejecting malformed
  or operator-mismatched tokens; `MutantOutcome` → pure Core `src/outcome.rs`; distinct crate-root vs
  target-file paths with `crate_root_of` correctly homed in the `cli.rs` adapter; pipeline-owned
  baseline green-check proven to abort *before any write*, both red and timed-out branches;
  splice/guard/runner still unfused). Module split (`outcome`+`operators` Core / `report`+`pipeline`
  Application) is the minimum honest decomposition — nothing pre-empts a later slice; `pipeline` takes
  domain primitives, no `&Cli`, and `RunConfig` correctly still does not exist.
  - **Recorded deviation (endorsed, not a defect):** `pipeline` (Application) `use`s `apply::RestoreGuard`
    and `runner::TestRunner` (Infrastructure) directly — an outward dependency normally inverted with a
    port trait. Sanctioned by `docs/design.md` ("narrow seams named at the boundary" + no premature trait
    seams). The pressure that would justify inverting it is **T16** (worker abstraction), not now.
  - **Read-only-target test technique — endorsed** over a fake-writer trait seam: it tests the real
    `RestoreGuard` against the real fs (a stronger proof than a fake), and the `writes_blocked` probe
    degrades honestly on root containers (weaker, never wrong) instead of false-passing.
  - **T13/S6 generalization cost did NOT rise.** `replacement(Operator, &str) -> Result<String>` is
    token-in/token-out with exactly **one** call site, and `splice` is already `(original, span, repl)`.
    T13 adds `Vec<Edit>` + a `splice_all` applying edits in **descending start order**; `replacement`
    survives as the single-token fast path. Blast radius: `site.rs`, `apply.rs` (+1 fn), one block in
    `pipeline.rs`; no public contract break.
  - **Visibility nit (fold into an S8 cleanup, not a churn commit):** `outcome` had to be `pub` only
    because `runner::classify` is `pub` — the lib's public surface is accidental (`apply`, `runner`,
    `scanner`, `site`, `manifest` are `pub` though only `cli`+`version` are reachable from `main.rs`).
    T9's own modules got this right (`mod operators/pipeline/report`).
  - **Assumption rulings:** (a) `DEFAULT_MUTANT_TIMEOUT = 300s` is the right erring-long call (a false
    timeout inflates the score under A8); (b) crate root = innermost `Cargo.toml` ancestor is correctly
    layered — note `fs::canonicalize` yields `\\?\`-prefixed paths on Windows, harmless for `cargo` but
    relevant to a custom `--test-command` (T17); (c) floats are a **taxonomy gap, not a bug** — no float
    row exists in any slice → product decision below; (d) `0b0000_0001 → 0b0` is mutant-*formatting*
    divergence with zero semantic effect (the mutant never reaches a diff or the manifest) — acceptable.
  - **`MutationReport` — the one substantive finding:** three `usize` counters are faithful to
    `docs/design.md` and A8, and `score() -> Option<f64>` on a zero denominator is right, but **a
    survivor count with no survivor locations is not actionable**, and **T12/S5 must *list* uncovered
    sites (A6), which a counter cannot do**. Report shape must carry per-mutant records by S5 → product
    decision below. `summary()` is a **change-detector, not a contract** (same ruling as `--scan` in T6);
    D4's JSON must serialize the model, never re-parse `summary()`.
  - **S3 acceptance — met.** T7 (apply/restore) + T8 (runner/timeout/classify) + T9 (parity operators +
    reporter), wired end-to-end through the CLI with C11 intact. Operator set is exactly the parity
    table — `/`, `%`, compound-assignment absent from **both** scanner and operators, so S4 has no
    partial state to unwind.
  - **T10/S4:** `every_mapping_changes_the_token` is the guardrail to extend — every new operator row
    must be added to it; a mapping returning its input produces an unkillable equivalent mutant.
  - **T12/S5:** land the per-mutant record shape here (counters become derived, `summary()` becomes a
    rendering) rather than bolting a parallel list onto `MutationReport`.
  - **T13/S6:** extract `mutate_sites`' per-site body into `mutant_source(original, site)` first, so the
    model generalization has one obvious point.
  - **T15/S7:** `--lines` filters the **pipeline's site list**, not the scanner — a filter applied
    between `scan_source` and the loop (`mutate_sites` iterates `sites` verbatim today).
  - **T16/S8:** both `RestoreGuard`'s target **and** `crate_root` must point at the worker's isolated
    copy; `crate_root_of` currently resolves against the canonical source and must be re-based.
  - **T17:** make `check_baseline` **return the measured `Duration`** (it is already the measurement
    point) and derive `timeout = baseline × --timeout-factor` (default **10**) — retires
    `DEFAULT_MUTANT_TIMEOUT` with no new plumbing. Also wire `--test-command` (replaces
    `runner::default_command()`), `--verbose` (the loop is currently silent for the whole run), and
    `--mutation-warning`.
  - **T17 — process-tree / job-object kill still owed (from T8):** `child.kill()` reaps only `cargo`,
    leaving a hung grandchild test binary holding the `target/` build lock and stalling the *next*
    mutant in single-worker mode. Not fixed by T16 isolation.
- **T10 — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S4 CLOSED — clear to proceed to S5/T11.** Clean,
  minimal slice: 7 rows added to a data-shaped enum + one match arm each across three Core files, with
  **zero churn** in `pipeline`/`apply`/`report`/`runner`/`cli`. Parity rows preserved byte-identical and
  now guarded by a **non-tautological regression test rather than an exclusion assertion** — the right
  inversion. All five T9 carry-forwards landed.
  - **Zero pipeline change is genuine, not coincidence** — `mutate_sites` is parametric over `Operator`
    and never over `SiteKind`, and `replacement` is total, so any *one-token → one-token* mutation flows
    through **by construction**. That is the T9 decomposition paying out, and it pins the boundary: the
    first non-single-token operator (T13) is the first that will touch `pipeline`. S4 was the correct
    free-rider; **S6 will not be.**
  - **`SiteKind::CompoundAssignment` — correct axis, keep it.** `SiteKind` is a taxonomy classifier
    (derived via `Operator::kind()`, living in Core, authored by the feature file's class rows), not a
    presentation concern; `+=` genuinely is a different site class from `a + b` (different arity of
    effect, different equivalent-mutant profile, and S6 will reason about assign-forms as a class).
  - **Equivalent-mutant guardrail is sufficient *as a guardrail*.** `every_mapping_changes_the_token`
    proves no mapping is a syntactic no-op; it cannot prove semantic inequivalence (operand-dependent —
    `x *= 1`, `x %= 1` — and undecidable in general). S4 owes nothing more **in code**; what it raises is
    the stakes on T12's per-mutant records, since an equivalent mutant surfaces as an unactionable
    location-less **Survived**.
  - **R3 sharpened:** `/→*` *removes* a divide-by-zero (safe direction), but `*=→/=` **guarantees a new
    division site at every `*=`**, `%=→*=` adds more, and all five compound ops inherit debug-mode
    overflow panics. Under A8 each folds into **Killed** — score inflation with zero signal about test
    quality, now systematic rather than incidental. Faithful to the accepted posture, so not blocking,
    but it materially strengthens **D6** → human note below.
  - **A5 unchanged in kind, sharpened in degree** — the reconciliation note already names the cause.
    But a derived consequence is uncosted: `--mutation-warning`'s default **50 is a parity-inherited
    constant applied to a non-parity site count**, so mutate4rust now trips it earlier than mutate4go on
    identical source (S6 widens this again). → T17.
  - **Keep the `match`; do NOT go data-driven.** 7 more variants are 7 more compiler-checked rows, and
    `Zero`/`One` already escape any table (they call `mutate_int_literal`), as will S6's structural rows —
    a table would immediately need an escape hatch and you would maintain both.
  - **S3-close decisions have clean ground:** `float_literals_are_not_constant_sites` is a **scope pin,
    not an obstruction** (T13 flips it exactly as T10 flipped `deferred_operators_emit_no_sites` —
    *invert, don't delete* is now the repo's scope-boundary idiom); `MutationReport` untouched with no
    parallel list bolted on.
  - **S1 carry-forward (fold into T13, or a 5-line change now) — `every_mapping_changes_the_token` is
    manually enumerated and therefore silently incomplete-able.** A new `Operator` variant forces a
    compile error in `replacement` and `kind()` but **not** in the guardrail test — the row is simply
    absent and the test still passes. Close it in Core: add `Operator::canonical_token(self) ->
    &'static str` (exhaustive match) + `Operator::ALL: [Operator; N]`, and iterate `ALL` in the test.
    `canonical_token` doubles as the rendering primitive T12 needs and retires the hand-kept pair list.
  - **T11/S5 — `Uncovered` becomes real against a widened site set.** Compound-assignment sites sit
    *inside statements*; region→line mapping (C8/R5) must be validated on a fixture where an `a += b;`
    line is covered, confirming the site's `line` is the key used against the coverage map and that a
    **multi-site line resolves consistently for all its sites**.
  - **T11/S5 — R8 is an *environment* risk, not a code one.** Add `llvm-tools-preview` +
    `cargo-llvm-cov` to `.github/workflows/ci.yml` **in T11, not T12**, and prove the **Windows leg
    first, not last**; a missing tool must fail loudly with an install hint, never silently mutate
    everything (A6).
  - **T12/S5 — the per-mutant record carries three things, not two:** `file:line` + operator/kind +
    outcome, **plus** enough to distinguish a *panic-killed* mutant from an *assertion-killed* one
    (bucket stays Killed per A8; the record gains fidelity).
  - **T12/S5 — make `Site.kind` a method, not a stored `pub` field.** It is fully derived from
    `operator`, has **zero non-test consumers** today, and the `pub` struct literal bypasses
    `Site::new`'s invariant. T12 is its first real consumer — convert to `pub fn kind(&self)` there
    rather than growing a second reader of derivable state (folds into the S8 visibility cleanup).
  - **T13/S6 — bitwise assign-forms are pinned but untabled.** `&=`, `|=`, `^=`, `<<=`, `>>=` are
    asserted to emit no sites, but the S6 taxonomy lists only the non-assign bitwise mappings. Either add
    the assign rows to the taxonomy or state explicitly that bitwise-assign stays out of MVP — don't let
    a test be the only place the decision lives.
  - **T17 — `--mutation-warning` default 50:** keep it and document the divergence, or scale it. A
    conscious call, not a silent parity inherit.

- **T11 — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S5 NOT closed — T12 outstanding.** Both S4-close
  slice items discharged: R8 landed in CI on **both** legs at T11 (not T12), and A6 was **verified against
  upstream source**, not guessed. The Core/Infra cut is the cleanest in the feature so far, and the fixture
  is the right kind of evidence — a real capture preserving `[3,25,5,6,3,…]`, the one region shape that makes
  the naive start-line mapping wrong.
  - **1. Layering — correct, and the serde DTOs are on the right side.** `coverage_map` is genuinely pure
    (no `std::fs`/`std::process`/`serde`; imports nothing), `coverage` is the only module touching process/fs,
    and Core is clean in **both** directions (`site`/`scanner`/`operators`/`outcome` import no coverage
    module). `Export`/`ExportData`/`ExportFunction` are private DTOs modelling an **external tool's wire
    format** — a versioned LLVM document — so they belong in Infrastructure by definition, and
    `parse_export(&str, &Path) -> Result<CoverageMap>` is a textbook anti-corruption translation: the
    dependency points **inward** (infra → Core type), never outward. The temptation to promote `parse_export`
    to Core because it is pure must be resisted — purity is not the test; **knowledge of a foreign format**
    is. Seams take domain primitives (`&Path`, `&[String]`, `&[OsString]`), never `&Cli`; `RunConfig` still
    correctly does not exist. The JSON-over-`--lcov` choice is right and correctly reasoned: lcov has already
    applied *llvm-cov's* collapse rule, which would make C8/R5 unfalsifiable rather than solved.
  - **2. Region→line rule — faithful, and `is_line_covered`'s shape is right for T12.** `count > 0 &&
    start_line <= line <= end_line` is byte-for-byte upstream `coverage.Covered`, and the two properties the
    doc names (regions span lines; any-covered-wins is monotone) are the correct ones. Monotonicity also
    silently buys the **generic-instantiation** case for free — a generic fn appears once per instantiation
    with independent counts, and any-wins is the right fold. Empty map ⇒ everything uncovered matches
    upstream's absent-profile behaviour. The signature `(&self, line: usize) -> bool` is the right shape:
    line-keyed, `Site`-free (so Core doesn't grow an intra-Core coupling), owned-by-caller, and — the part
    that matters — **it keeps the representation open**: the linear scan is O(sites × regions), fine at MVP
    scale, and if profiling ever bites, a sorted interval index drops in behind the identical signature.
    Do **not** optimize now (YAGNI).
    - **T12 alignment worth exploiting:** covered-only gating and `--lines` (T15) are *both* line-keyed
      predicates over the pipeline's site list, applied between `scan_source` and the mutate loop. Land
      **one** composition point in T12 that T15 extends, not two independent filters.
  - **3. The `coverage_command`/`run_coverage` seam — justified; it is not the T9 fake-writer.** T9 rejected
    a **polymorphic trait** introduced solely to let tests substitute a fake for the real fs. This is
    categorically different: it is a **parameter**, not an abstraction. The real `Command`/`Stdio`/exit-status/
    stderr code path executes in the tests, against real processes — the same "test the real thing" principle
    that made the read-only-target technique the right call. It also matches the repo's own established
    precedent, `TestRunner::new(command, …)` + `runner::default_command()`, so T11 introduces no new idiom.
    `a_coverage_run_that_fails_reports_the_status_and_the_stderr` is the strongest test in the change: a
    digit-free script name plus full-equality against a message rebuilt from a *real* probe run means it
    cannot be satisfied by an incidental substring and cannot drift across platform `ExitStatus` renderings.
    Endorsed.
    - **Nit:** `backend_probe() -> Vec<String>` vs `coverage_command() -> Vec<OsString>` — two vocabularies
      for one concept inside one module. Prefer `OsString` for both (paths force it); a five-line change,
      fold it into T12 rather than a churn commit.
    - **Do NOT extract a shared spawn helper yet.** `run_coverage` and `TestRunner::run` both render+spawn,
      but with genuinely different semantics (capture vs `wait_timeout`+kill) and different error contracts.
      Two call sites is not DRY pressure. Revisit only if T16/T17 introduces a third spawner.
  - **4. `pub` visibility — tighten in T12, and T12 is the *first moment it can compile clean*.** The
    profile's least-privilege rule does argue for `pub(crate)` now, and there is no external consumer (no
    `tests/` directory exists; `main.rs` reaches only `cli`+`version`), so this is pure accidental surface.
    But tightening **today** would fail the `-D warnings` gate: `generate`, `reuse`, and
    `CoverageMap::is_empty` have **zero non-test callers** while `pipeline.rs` is untouched, so `pub(crate)`
    turns them into `dead_code`. The honest sequencing is therefore **not** "fold into S8" — it is: tighten
    `coverage`, `coverage_map` and their items to `pub(crate)` **in T12**, at the exact commit that gives
    them real callers, and leave the *pre-existing* accidental surface (`apply`, `runner`, `scanner`,
    `site`, `manifest`, `outcome`) to the S8 sweep. Record it as a T12 acceptance item so it is not lost
    between the two.
    - **Standing profile gap (pre-existing, not T11's):** the project profile mandates
      `#![forbid(unsafe_code)]`; `src/lib.rs` carries no such attribute and never has. One line, zero risk —
      add it in T12 or the S8 sweep, but stop carrying an unmet stated convention.
  - **5. Format-drift guard is one notch weaker than the tests claim.** `a_short_region_tuple_is_an_error`
    asserts "a format change must fail loudly", but the length check only catches **truncation**. A field
    **reorder** in a future export version would be read silently at the wrong indices and could report a
    covered file as uncovered — the exact silent-wrong-answer R8 exists to prevent. Cheap close in T12: read
    the export's `type` (`llvm.coverage.json.export`) and `version` major, and fail with a named message on
    an unexpected value. The fixture already pins 3.1.0 in its doc comment; make the code assert what the
    comment claims.
  - **6. Region `kind` (tuple index 7) is ignored — safe direction, but say so deliberately.** The tuple's
    trailing field distinguishes Code / Expansion / Skipped / Gap / Branch / MC-DC regions, and the parse
    treats them all alike. Zero-count kinds are harmless (the rule is monotone — they can never *remove*
    coverage), and a non-zero-count Gap/Expansion region can only *over*-mark a line as covered, i.e. we
    mutate a site we might have skipped and report the survivor. That is the **correct direction to err**:
    over-reporting is visible, under-reporting is silent. But it means our numbers may not match
    `cargo llvm-cov report`'s own line percentages (llvm-cov excludes gap-only lines). Document the
    acceptance in the module header, or filter to kind 0 — either is fine; leaving it undocumented is not.
  - **7. Test gaps I would actually ask for (T12).**
    - **`reuse`'s success path and `load`'s read path have zero coverage** — only the absent-profile error is
      tested. Writing `FIXTURE_EXPORT` to `profile_path(dir)` and asserting `reuse` returns the same map as
      `parse_export` is ~8 lines and closes the only untested production path in the module.
    - **The CI install is currently unearned.** Both legs now install `llvm-tools-preview` +
      `cargo-llvm-cov` and verify `--version` — but **no test in the suite invokes the backend**, so we pay
      the install on every run and prove nothing beyond presence. T12 must land the feature file's own
      guidance — the end-to-end tiny-fixture-crate run (discover → coverage → mutate → test → report) with a
      known covered/uncovered outcome. That is also the **first real proof of the Windows leg**, which the
      S4-close item asked for "first, not last". Keep it to one such test (golden rule #8: don't overdo).
    - Minor: no test exercises a **non-zero `file_id`** (multi-file function / macro expansion); the fixture
      has single-entry `filenames`, so the indexing logic is only proven on the error path.
  - **8. Carry-forwards to T12 — confirmed, with two additions.**
    - **Upstream's stdout notice is owed:** `"Reusing existing coverage; covered/uncovered classification may
      be stale."` on the `--reuse-coverage` path. Verified present upstream, deliberately not implemented at
      T11. It is parity, and it is the user's only signal that the gate may be lying to them. Not optional.
    - **`Site.kind` → method** (from T10): T12 is its first real consumer; convert `pub kind` to
      `pub fn kind(&self)` there rather than growing a second reader of derivable state.
    - **`Operator::canonical_token()` + `Operator::ALL`** (from T10): still owed, and T12 raises the payoff —
      `canonical_token` is exactly the rendering primitive the per-mutant record needs, and `ALL` makes the
      compiler enforce `every_mapping_changes_the_token` instead of trusting a hand-kept list.
    - **Per-mutant records with panic-killed distinguishable:** unchanged and now urgent — an uncovered-site
      **list** (A6) is not expressible by a counter, and T10 sharpened the equivalent-mutant problem into a
      location-less `Survived`. Land the record shape in T12 with counters becoming derived and `summary()`
      becoming a rendering; do not bolt a parallel list onto `MutationReport`.
    - **NEW — the double compile before the first mutant (R1).** A default run will now do a `cargo llvm-cov`
      pass **and** `pipeline::check_baseline`'s `cargo test`. `cargo-llvm-cov` builds instrumented into its
      own target directory, so the coverage pass does **not** warm the cache the mutate loop uses: two full
      compiles before mutant #1. Both are defensible (the baseline must be measured on a *non*-instrumented
      build — T17 derives the timeout from it — and a green `llvm-cov` run is not a substitute), so this is a
      cost to **acknowledge and order correctly**, not to eliminate: coverage first, against pristine source,
      before `RestoreGuard` takes its copy. `--reuse-coverage` is the user's lever. Record it under R1.
    - **NEW — the file's own `#[cfg(test)] mod tests` is a coverage-gating landmine.** The scanner performs
      no `cfg`-attribute filtering (`visit_item_mod` recurses unconditionally), and test-module lines are
      **always covered by construction** — the fixture proves it: regions `[18..20]` carry count 1. So
      covered-only gating will *preferentially* mutate a file's own test code while skipping genuinely
      uncovered production code. Upstream never faced this: Go tests live in a separate `_test.go` file that
      the one-file-at-a-time model simply never selects. **This is a real C-class conflict absent from
      C1–C10** and it is a product decision, not mine: (a) skip `#[cfg(test)]` items at discovery, (b) mutate
      them and accept the noise, or (c) warn. My recommendation is (a) — a mutant of an assertion constant is
      killed by its own test, costs a full compile, and carries no signal about test quality. → human.
  - **9. Path matching (`normalise`-then-suffix) — the matching rule is fine; the *silent* failure is not.**
    Suffix matching is the correct port of upstream and the separator normalisation is right. Case is a real
    hole on Windows: a user typing `SRC/Lib.rs` opens the file fine (case-insensitive fs), scans fine, and
    then matches **nothing** in the export — yielding an empty map, every site `Uncovered`, zero mutants run,
    and **exit 0**. A silent wrong answer is precisely what R8/A6 were written to forbid. But blanket
    case-folding is the wrong fix — it would be incorrect on Linux. Ruling: **not an accepted limitation, and
    the fix is not case-insensitivity.** T12 must land the backstop, and should prefer the precise fix:
    - **Mandatory (T12):** when `CoverageMap::is_empty()` after a *successful* coverage generation, do not
      report "everything uncovered" — that outcome is far more often a path mismatch than a genuinely
      untested file. Fail (or warn loudly) naming the target and the profile, listing what the export *did*
      contain. `is_empty()` already exists for exactly this; give it its production caller.
    - **Preferred (T12):** canonicalise the target with `std::fs::canonicalize` before matching — llvm-cov
      emits absolute native paths, and on Windows canonicalisation resolves to the on-disk casing, making the
      comparison platform-correct rather than platform-guessed. Keep the suffix rule as the fallback when
      canonicalisation fails (note the `\\?\` prefix caveat already recorded at T9). Keep normalise-then-
      suffix; add exactness where the OS can supply it.
  - **10. A6 verification — accepted as discharged.** Upstream `ensureCoverage` erroring when
    `--reuse-coverage` is set and `LoadProfile` returns `nil, nil` on `os.IsNotExist` is a genuine source
    verification, and `target/coverage/coverage.out` → `target/coverage/coverage.json` is a port of the
    location, not an invention (A2 intact). The provisional S4-close answer and upstream agree; record that
    they agreed, which is worth more than either alone.
  - **Endorsed, don't second-guess:** the `coverage` tests importing `crate::scanner`/`crate::site` is an
    **inward** dependency and is what makes `every_site_on_a_multi_site_line_resolves_consistently` a real
    proof rather than an assertion about a hand-built map — it discharges the T10 carry-forward exactly as
    written, with pinned site counts (4 on line 4, 2 on line 11) so it cannot degrade into a single-site
    tautology. Fixture reduction is honest and documented (region tuples verbatim; only unconsumed bulk
    stripped). `pipeline.rs` left untouched is the right call — nothing consumes the map yet, and a
    speculative wiring would have been the premature move.

- **T12 — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S5 CLOSED — clear to proceed to S6/T13.** The
  slice's five stated outcomes all land (covered-only execution; uncovered reported-and-skipped;
  `--reuse-coverage` incl. the verbatim upstream notice; A6 coverage-absent parity; `cargo-llvm-cov`
  wired end-to-end), and **all four T11-close human decisions are discharged in the same commit**:
  `#[cfg(test)]` skipped at discovery, `taiki-e/install-action` pinned to
  `b6ff580856c41316412a0b9b60540fbc6f8c82cc # v2.86.7`, `coverage`/`coverage_map` and their items
  tightened to `pub(crate)`, `#![forbid(unsafe_code)]` added. Nothing is owed for S5.
  - **The record model is right, and it is right for the right reason.** `MutationReport` stores
    `file` + `Vec<MutantRecord>` and *nothing else* — every counter is an `iter().filter().count()`,
    so there is no parallel tally that could drift, and `the_counters_sum_to_the_record_count` pins
    the partition rather than a hand-computed number. `MutantResult::Killed(KillReason)` with
    `bucket()` folding all three reasons into `Killed` is the exactly-correct expression of A8: the
    record gains fidelity, the bucket set is provably unchanged
    (`every_kill_reason_folds_into_the_killed_bucket` iterates all reasons). D6 now has the
    instrumentation it was deferred pending. **A3 single-file → file-on-report, not per-record** is
    the right normalization and the one that will make D4's JSON smallest.
  - **Arithmetic-panic detection by rustc's three specific messages is the only defensible rule** —
    a generic `"panicked"` detector would match every `assert!` and destroy the R3 signal.
    `an_assertion_panic_is_not_an_arithmetic_panic` is the test that makes it honest. Accepted risk,
    correctly bounded: the markers are rustc message strings and can drift across toolchains; drift
    degrades an `ArithmeticPanic` into a `TestFailure` — a **fidelity** loss inside the correct
    bucket, never a bucket error. That is the safe failure direction; no action.
  - **`macro_rules! declare_operators` — endorsed, and the right fix for the class of bug found.**
    The round-1 FAIL was real: a hand-kept `ALL` makes "the guardrail is complete" an unenforced
    convention. Generating the enum and the roster from one variant list makes the omission
    *inexpressible* rather than merely tested-for, which is the stronger construction. This is the
    repo's second instance of the same principle (derive-don't-store, after `Site.kind`) and it should
    be treated as the house idiom.
    - **Q5 ruling — yes, size-pinning is the correct residual assertion.** Membership and order are
      guaranteed by construction, so asserting them would be a tautology over the macro. What
      construction *cannot* guarantee is that a roster change was deliberate; `Operator::ALL.len() == 22`
      is precisely the residue. Expect to bump it at T13 — that is the test working, not failing.
  - **`Site.kind` → method, `canonical_token()`, `OsString` unification, region-`kind` documentation,
    the export `type`/`version`-major drift guard, the `reuse`/`load` success-path tests** — every T11
    carry-forward landed as written. The drift guard in particular closes the one-notch-weak finding:
    the fixture's doc comment claimed 3.1.0 and now the code asserts it.
  - **`ensure_not_empty` — the right backstop, and the reuse-path exemption is the right nuance.** An
    empty map after a *successful* generation is far more often a path mismatch than an untested file,
    and the message names the target, the profile, "usually a path mismatch", and every filename the
    export contained — that is diagnosable at a glance rather than a bare failure. Deliberately not
    applying it under `--reuse-coverage` is correct: a stale profile may legitimately omit the file,
    and the reuse notice already warns. `fs::canonicalize`-preferred with normalise-then-suffix
    fallback, memoised per distinct filename, closes the Windows-casing hole precisely rather than by
    blanket case-folding. Item 9 of the T11 review is fully discharged.
  - **`#[cfg(test)]` divergence — correct, correctly documented, correctly bounded, but not applied
    uniformly.** The literal-only match (`cfg(test)` exactly; `cfg(any(test, …))` still scanned) is
    the right conservatism — a composite predicate compiles outside test builds, so treating it as
    test-only would silently drop production sites — and
    `a_composite_cfg_predicate_is_still_scanned` pins it. The control-vs-attribute pairing in
    `sites_inside_a_cfg_test_module_are_not_discovered` (identical body scanned without the
    attribute) is the right proof shape: it shows the *attribute* suppresses discovery, not the
    nesting.
    - **Gap worth closing at T13 (not a blocker):** `is_cfg_test` is checked in `visit_item_fn`,
      `visit_impl_item_fn`, `visit_item_impl`, `visit_item_mod` — but **not** in `visit_item_trait`
      or `visit_trait_item_fn`, and not for module-level `#[cfg(test)] const`/`static` (whose
      literals still reach `visit_lit_int`). Low-frequency, but the shape of the omission is the
      concern: a per-visitor gate is a hand-kept list, and hand-kept lists are exactly what
      `declare_operators` was just introduced to abolish. **Preferred fix at T13:** gate **once** in
      `visit_item` via an exhaustive `item_attrs(&syn::Item) -> &[Attribute]` match — a new `syn::Item`
      variant then becomes a compile error instead of a silent hole, and the six per-visitor checks
      collapse to one. Same principle, applied to the scanner.
  - **Q1 ruling — the summary contract is right; the format has one misread risk.** "Change-detector,
    not a stability contract pre-1.0" is the correct call and consistent with the identical rulings at
    T6 (`--scan`) and T9. **D4 must serialize records and must never re-parse `summary()` — endorsed
    without reservation**; that is the whole reason records-as-model was mandated. On the format
    itself:
    - Always printing all three kill reasons (including zeros) — **keep**; greppable and
      shape-stable.
    - `Score: 100.0%` on a run that mutated 1 of 2 sites is the one line a human will misread as
      "this file is well tested". The number is correct per design.md (uncovered excluded, reported
      separately) but it is presented without its denominator. **Suggest** qualifying the line
      (e.g. `Score:     100.0% (1 of 1 mutants run; 1 site uncovered, excluded)`) — cheap, and it
      makes the exclusion impossible to miss. **→ human's call on wording.**
    - `records()` returns insertion order, which is *uncovered-first-then-mutated*, not source
      order. Within each listing the order is source order (partition is stable), so the human-facing
      output is fine — but D4 should sort by line at serialization rather than inherit an artifact
      of the loop.
  - **Q2 ruling — split the two.** `MutationReport::records()` is a legitimate accessor on the model
    that D4 will need; `#[cfg(test)]` on it today is the honest way to satisfy `-D warnings`, and
    un-gating is a one-line change at its first production caller. Keep it. `parse_export` is a
    different animal: it is a *test-scaffolding wrapper* (`map_for(&parse_document(json)?, target)`)
    living in the production namespace with no production role. **Move it into `mod tests`** — same
    two lines, zero production surface, and the module then contains only code the product runs.
    Non-blocking; fold into T13.
  - **Q3 — `report.killed_by` `pub(crate)` is correct.** It has a real production caller (`summary`)
    and no external one. No finding.
  - **Q4 — accepted, but the waste is avoidable without touching R1 ordering.** Coverage must stay
    ahead of `RestoreGuard` (a coverage pass over mutated source classifies the wrong program), and
    that is not negotiable. But an unparseable target can be rejected by a **millisecond
    `syn::parse_file` pre-flight on the target's bytes before the coverage run**, discarding the
    result. That is a *validation*, not a stage, so it reorders nothing and costs nothing — it just
    stops a ~30 s instrumented build being spent to learn the file has a syntax error. **Suggest for
    T13**, where the scanner is already being touched.
  - **Q — the `Selection`/`select_sites` composition point: shape is right, arity is not, and T15
    will feel it.** Landing **one** line-keyed filtering point between `scan_source` and the loop was
    the correct T11 mandate and it is correctly discharged; borrowing the scan (`Vec<&Site>`) rather
    than cloning is right; `site.line` as the whole coverage key keeps multi-site lines consistent.
    But the current two-way partition encodes an assumption that **every site is either mutated or
    Uncovered**, and that is exactly what `--lines` (T15) and differential selection (T14) break:
    - a covered site **outside `--lines`** is *not uncovered*. Recording it as `Uncovered` would be a
      false statement about the test suite and would corrupt the one bucket S5 exists to produce.
    - `MutantOutcome` must **not** grow a fourth bucket to absorb it — that would break A8/parity.
    - **Ruling: filters narrow, gating classifies.** `--lines` and differential selection are
      *site-list narrowing* applied **first**; a narrowed-out site produces **no record at all**.
      Coverage gating then partitions only what survived narrowing, exactly as today. Concretely,
      `Selection` gains a third, *unrecorded* list (or `select_sites` takes a
      `filter: impl Fn(&Site) -> bool` applied before the partition), and the summary may report a
      narrowing count as a non-bucket line (`Sites skipped by --lines: N`). This is a ~15-line change
      at T15, not a redesign — **provided T13/T15 do not first start recording narrowed-out sites as
      Uncovered.** Land the three-way shape at the moment the second filter arrives, not before
      (YAGNI), but do not let the binary partition harden into an assumption.
    - **Owed verification at T15 (do not guess):** does upstream mutate4go apply `--lines` before or
      after coverage gating, and does it report line-excluded sites at all? Same discipline as the A6
      resolution — check the source, record what it does.
  - **Q — the stderr drain: dropping (not joining) on timeout is the right call, and it sharpens
    T17's owed work rather than duplicating it.** Joining would re-introduce precisely the hang the
    timeout exists to escape, because a hung *grandchild* still holds the pipe's write end open —
    the reasoning in the doc comment is correct and the flood test proves the concurrent drain
    prevents the pipe-full deadlock being misreported as a timeout. Two consequences to carry:
    1. **Dropping the `JoinHandle` does not close the read end** — the detached thread owns the pipe
       and stays blocked in `read_to_end` on an unbounded `Vec`. So each timeout leaks one thread
       *and* an unbounded buffer, and the still-open read end means the hung grandchild never gets a
       broken pipe to stop it. This is the **same root cause** as the known Windows
       orphan-grandchild problem owed to T17: a real process-group (Unix) / job-object (Windows)
       kill closes the write end, the reader hits EOF, the thread exits and the buffer frees. **T17's
       process-tree kill therefore also retires this leak** — record it as a second reason that item
       is owed, not merely a nicety.
    2. **Independent of T17, bound the buffer.** `pipe.take(CAP).read_to_end(..)` with a generous cap
       (~8–16 MiB; the flood test captures 512 KiB, so no test moves) makes memory bounded on *every*
       path, including the success path where a mutant that loops printing to stderr for 299 s can
       balloon. Two-line change; **suggest for T17** alongside the tree-kill.
    - Discarding stderr entirely on the timeout path is correct — `classify` gives `Timeout` absolute
      precedence over any stderr content, so nothing is lost.
  - **`tests/e2e.rs` — the right test, correctly scoped, and it finally earns the CI install.** One
    real fixture-crate run proving discover → coverage → mutate → test → report compose (Killed 1 /
    Survived 0 / Uncovered 1, uncovered listed by location, target pristine afterwards, both the
    generate and the reuse path), plus a sub-second exit-code assertion that compiles nothing. Golden
    rule #8 respected — one heavy test, not a suite. The **env scrub** is the load-bearing detail and
    is correct: without removing `RUSTFLAGS`/`CARGO_TARGET_DIR`/`LLVM_PROFILE_FILE`/`CARGO_LLVM_COV*`
    the nested instrumented build would inherit the outer `cargo llvm-cov` context and either reuse
    the wrong target dir or refuse to instrument — a failure that would have looked like a product
    bug. Counting uncovered *entries* rather than grepping a line number, because the mutant's own
    `cargo test` output is inherited into stdout, is the honest workaround.
    - The workaround points at a **real UX defect owed to T17**: the runner inherits child **stdout**,
      so a default run floods the terminal with one full `cargo test` transcript per mutant, and the
      report is buried at the end. **T17 must suppress mutant stdout by default and surface it under
      `--verbose`** — the flag is already reserved for exactly this, and it makes the report readable.
      (Keep the baseline's stderr capture as-is; it is what makes the red-baseline abort
      diagnosable.)
  - **Minor, non-blocking:** `mutate_with_coverage` runs `check_baseline` even when
    `selection.mutate` is empty — a full non-instrumented compile spent to validate a run with zero
    mutants. Pinned by `every_site_is_uncovered_when_the_map_is_empty_but_the_baseline_still_runs`,
    so it is deliberate. Defensible today (T17 will want the baseline *measurement* regardless), but
    once `--lines` can narrow to zero it becomes a visible waste — revisit at T17 when the baseline
    gains its timing role.
  - **Carry-forwards to T13–T17**
    - **T13 (highest priority) — the record's rendering assumes one-token operators.**
      `MutantRecord::render` prints `Operator::canonical_token()`, and `canonical_token` is defined as
      "the operator's own source token". S6's structural operators (`Some(x) → None`,
      `expr? → expr.unwrap()`, arm-body swap) **have no canonical token**, and the T4 carry-forward
      already predicted the single-span/single-`Operator` `Site` will not fit them. When the model
      generalizes, the record must carry a **description of the mutation** (or the original and
      replacement slices), not a token — and `canonical_token` should narrow to what it truly is: the
      guardrail/round-trip primitive for single-token operators. Decide this *with* the `Vec<Edit>`
      change, not after; retrofitting the record shape twice is the avoidable cost.
    - **T13 — gate `#[cfg(test)]` once in `visit_item`** (above), and bump
      `all_holds_the_declared_operator_roster` deliberately.
    - **T13 — move `parse_export` into `mod tests`**; pre-flight-parse the target before the coverage
      run (Q4).
    - **T14/T15 — `Selection` becomes three-way** (narrow → then gate), with narrowed-out sites
      producing **no record**; no fourth bucket. Verify `--lines`×coverage precedence against upstream.
    - **T15 — records will want `function_id`.** They deliberately carry only `line`; differential
      reporting ("which functions still have survivors") needs the function. Cheap to add then; noted
      so it is not rediscovered as a defect.
    - **T16 — unchanged and still correct:** both `RestoreGuard`'s target and `crate_root` must point
      at the worker's isolated copy; `crate_root_of` still resolves against canonical source. Add:
      **coverage generation must not be run per-worker** — one pre-run coverage pass feeds all workers,
      or N instrumented builds will dwarf the mutation cost.
    - **T17 — process-group / job-object kill is now owed for two reasons** (orphan grandchild holding
      the `target/` build lock **and** the leaked unbounded drain thread); bound the stderr buffer;
      suppress mutant stdout unless `--verbose`; `check_baseline` returns the measured `Duration` and
      `timeout = baseline × --timeout-factor` (default 10), retiring `DEFAULT_MUTANT_TIMEOUT`; wire
      `--test-command`, `--mutation-warning` (and rule on the parity-inherited default 50 against a
      non-parity site count — now *reduced* by the `#[cfg(test)]` skip, which cuts the other way and
      should be re-measured after S6).
    - **S8 visibility sweep — unchanged scope:** `apply`, `runner`, `scanner`, `site`, `manifest`,
      `outcome` remain accidentally `pub`. T12 correctly tightened only what it gave callers to.
  - **Nothing in T12 compromises the architecture if carried forward.** Core stayed pure (`site`,
    `scanner`, `operators`, `outcome`, `coverage_map` import no fs/process/clap; the macro is a
    declaration-site device, not a dependency); the only outward edge is the already-sanctioned
    `pipeline → apply`/`runner`/`coverage`; seams still take domain primitives and `RunConfig`
    correctly still does not exist — **T15 is the slice that finally justifies it** (`--lines`,
    `--since-last-run`, `--mutate-all` arriving together), and it should be introduced there rather
    than retrofitted out of `&Cli` coupling.

### ⚠ Product decisions raised by Anders at T12 close / S5 close (see RESOLVED section below)
1. **Score-line qualifier.** Does `Score: 100.0%` carry its denominator and the uncovered exclusion
   inline (e.g. `100.0% (1 of 1 mutants run; 1 site uncovered, excluded)`), or stay bare? The bare
   form is correct per design.md but invites a "fully tested" misread on a partially-covered file.
2. **Mutant stdout under `--verbose` (T17).** Confirm that suppressing per-mutant `cargo test` output
   by default is wanted — it is a UX divergence from today's inherit-everything behaviour, and it
   changes what a CI log looks like.
3. **`--lines` × coverage precedence (T15).** Confirm Anders' ruling — line-excluded sites are
   **not reported at all** (not `Uncovered`, no fourth bucket) — pending the upstream check.

- **T13a — APPROVE-WITH-SUGGESTIONS** (no blockers) · **S6 NOT closed — T13b/T13c outstanding.** The three
  token-level operator classes land cleanly on the existing model, the roster grows 22 → 33 with
  membership still guaranteed by construction, and the two T12 housekeeping items plus the score-line
  human decision are discharged. The slice's real value is not the eleven operators — it is that
  splitting T13 exposed a **live production defect** (`0f64`) that would have been invisible under
  operator noise. Everything below is carry-forward or scope-drawing; nothing is owed inside T13a.
  - **1. The T13a/T13b split — RIGHT CALL, but T13b's scope is drawn on a premise that does not hold.**
    Deferring the model change until its justifying pressure arrives is exactly the discipline this repo
    has applied five times already (`RunConfig` → T15, three-way `Selection` → T15, `pub(crate)` at the
    commit that gives callers, `Site.kind` → method at its first consumer, records at T12). The `0f64`
    find is the empirical vindication: a mapping that fails mid-run and **aborts a real mutation run**
    was sitting under the current model and would have been attributed to `Vec<Edit>` churn.
    - **But: only ONE of T13b's five operator classes actually requires `Vec<Edit>`.**
      `Some(x) → None` (one span, fixed replacement `"None"`), `Ok(x) → Err(x)` (one span, the `Ok`
      identifier), `expr? → expr.unwrap()` (one span, the `?` token), `.unwrap() → .unwrap_or_default()`
      (one span, the method identifier — *the identical shape T13a just shipped for predicate swaps*),
      and match-arm **guard drop** (one span, replacement `""`) **all fit today**. Match-arm **body
      swap** — two disjoint spans, each replaced by the other's text — is the **only** operator in S6
      that cannot be expressed as one edit.
    - What the other four need is not a multi-span model but the smaller generalization the T12 review
      already named: **the replacement stops being a function of the token**, and the record stops
      rendering `canonical_token()` and starts rendering a *mutation description*. `replacement` is
      already token-independent for 20 of the 33 operators.
    - **Ruling: re-draw the boundary.** `T13b` = the structural operators that fit the current model +
      the record-rendering generalization + the `Stmt`/`Arm` cfg gate — **no Core model change**.
      `T13c` = **arm-body swap**, the *sole* justification for `Vec<Edit>` + `splice_all` (descending
      start order), landing with its one justifying operator. Same split the human made at T13, applied
      one level down; it makes the model change a ~40-line commit with a single behavioural driver
      instead of a model change hiding behind five operators — precisely the failure mode the T13a/T13b
      split was created to prevent. Corollary: this makes arm-body swap **droppable** without stranding
      the rest of S6. Under the current drawing it is not.
    - **T12 carry-forward not yet discharged, and it is now step 1 of T13b:** extract `mutate_sites`'
      per-site body into `mutant_source(original, site)`. `pipeline.rs` is untouched by T13a (correctly —
      T13a is a pure free-rider on the T9 decomposition, exactly as S4 was), so the "one obvious point"
      for the generalization still does not exist. Land it **before** the first structural operator.
  - **2. The four-gate `#[cfg(test)]` design — CORRECT SHAPE; both deviations from the brief are upheld.**
    I was wrong on both counts and the coder's correction is the better construction.
    - The compile-error property demanded is **unattainable**, and the code proves it rather than merely
      asserting it: `_ => &[]` sits after an exhaustive variant list under `-D warnings`, and an
      `unreachable_patterns` warning would have failed the gate. The wildcard compiling clean **is** the
      verification that these enums are `#[non_exhaustive]`.
    - Four gates, not one, is forced: `visit_item` cannot see a `#[cfg(test)]` trait method or associated
      const, and the round-1 `ForeignItem` FAIL (`#[cfg(test)] static X: [u8; 1];` leaking its `1` via
      `visit_type_array → visit_lit_int`) proves the enumeration is real work, not ceremony. That the
      FAIL was found by a *test* and not by reasoning is the point.
    - `_ => &[]` meaning **keep scanning** is the right default — under-scanning production code is the
      worse failure (false confidence, silently) — but **the stated justification is one notch stronger
      than the truth**. "Over-scanning is visible" only holds if the wrongly-scanned mutant *survives*; a
      mutated assertion constant inside test-only code is killed by its own test and appears as an
      ordinary `Killed`, i.e. invisible **and** score-inflating. Keep the default; **soften the claim to
      "over-scanning costs a compile and inflates the score; under-scanning silently withholds signal —
      we prefer the former."**
    - **Dropping the universal completeness claim and naming what is *not* gated is the single best
      change in T13a.** A comment that enumerates its own holes is worth more than one promising
      coverage it cannot deliver. House style for every foreign-shaped boundary.
    - **Closer to enforcement? Exactly one construction, unavailable to us.** rustc's
      `non_exhaustive_omitted_patterns` lint fires precisely when a wildcard covers omitted variants of a
      `#[non_exhaustive]` **foreign** enum — written for this exact problem, still **unstable/nightly-only**
      and therefore unusable under the profile's stable-toolchain posture. Record it as the named future
      close (verify status before relying on it); build no bespoke substitute meanwhile.
    - **The general answer to "a hand-kept list is a silent hole", now hit three times — classify the
      list by who owns the domain.** This deserves a line in `docs/design.md`:
      - **We own the domain** (`Operator::ALL`) ⇒ make the omission **inexpressible** — generate the list
        and its consumers from one declaration (`declare_operators!`). Enforcement by construction.
      - **A foreign, `#[non_exhaustive]` domain** (`syn::Item` & friends) ⇒ enforcement is impossible by
        construction *and* by compiler. Instead: (a) choose the wildcard default whose failure mode you
        can live with, and say which; (b) enumerate explicitly anyway, so the audited set is readable in
        one place; (c) **make the test the enforcement** — a fixture table pairing each construct with
        its attribute-free control, which is exactly
        `cfg_test_suppresses_every_item_kind_that_can_hold_a_site`. That test, not the match, holds the
        property, and it is the artifact a syn bump must be re-run against.
      - Residual risk to accept knowingly: `syn = "3.0.4"` is a **caret** requirement, so a new variant
        arrives on a `cargo update` with no signal. An exact `=3.0.4` pin would convert that into a
        deliberate bump, at the cost of duplicate-syn hazards and no patch fixes. **Recommendation: keep
        the caret** and add "re-audit `scanner`'s four `*_attrs` matches" to the review checklist for any
        syn bump — the module header already names 3.0.4 as the audited version, which is half the job.
  - **3. The single-source float precondition — right for floats; do NOT generalize the shape.
    Generalize the *invariant* instead, with a property test.** `plain_decimal_float_value` shared by
    scanner and mapping is correct and well-placed (pure Core, one definition, consumed by the emission
    gate and the validator).
    - The two callers feed it **different inputs** — the scanner passes `lit.base10_digits()` (suffix and
      `_` already stripped by syn), the mapping passes a **raw source slice**. It tolerates both because
      it re-strips `_`; that is load-bearing and pinned by the `0.0_f32 → 1.0f32` row. Worth one sentence
      in the doc comment.
    - **Generalizing the shape to all 33 operators would be over-engineering** — 20 are total
      token-for-token swaps with no precondition to share. A shared-precondition function is warranted
      only where the emission decision and the mapping's validation are the *same non-trivial predicate*:
      floats (done), integer suffixes (done), and **every precondition-gated operator arriving in T13b**.
    - **The generalizable answer is the invariant, not the function: "the scanner emits a site ⟺ the
      mapping succeeds."** Today that is asserted only per-row, by hand, in the composition test — another
      hand-kept list, and exactly the list `0f64` was missing from. **Land one property test at T13b:**
      scan a corpus fixture including the awkward spellings and assert `operators::replacement(...)` is
      `Ok` for **every** emitted site. Eight lines; it would have caught `0f64` generically rather than
      after it aborted a run, and it scales to five more preconditions instead of five more hand-written
      rows. Pair it with `every_mapping_changes_the_token` — one guards totality, the other non-identity.
    - **Related:** a mapping failure in `mutate_sites` propagates `?` and **aborts the entire run,
      discarding the partial report** — what made `0f64` a run-killer rather than a skipped site. Keep
      the fail-fast (silently skipping would hide precisely this bug class), but **print the partial
      report before bailing**: a user 40 mutants into a 30-minute run should not lose all of it to an
      internal invariant violation. T13b or T17.
  - **4. Predicate-method swaps without type information — CORRECT for a syntactic tool. No guard.**
    Acquiring type information means `rustc`-as-a-library, a different product. The failure is bounded: a
    user-defined `is_some()` on a type with no `is_none()` produces a mutant that fails to compile and
    folds into **Killed** under A8. A name-based heuristic guard would be strictly worse — wrong in both
    directions, and a fake type system in Core.
    - **What it costs: the direction of the error is score *inflation*.** A non-compiling mutant is a
      `Killed` that says nothing about test quality. Tolerable at T13a's scale. **Not tolerable
      unmeasured at T13b**, where every remaining operator carries an undecidable precondition —
      `.unwrap() → .unwrap_or_default()` needs `T: Default`, `Ok(x) → Err(x)` a compatible error type,
      `expr? → expr.unwrap()` a permitting `Try` type. The taxonomy's "precondition-gated emission" rule
      **cannot be honoured for any of them** without types, so T13b will systematically manufacture
      non-compiling mutants and score them Killed.
    - **Recommendation (land at T13b, ahead of the structural operators): add
      `KillReason::CompileError`.** The record model exists for exactly this fidelity (T12), the detector
      is cheap and specific in the same style as the arithmetic-panic markers (`error[E…]` /
      `error: could not compile`), and the bucket set is provably unchanged. Without it we ship a class
      of operators whose cost we have no instrument to measure — and A5's owed "record the
      count-divergence magnitude once S6 lands" becomes unanswerable in the one dimension that matters.
      **Highest-value item in this review after finding #1.**
  - **5. The two deliberately-unfixed findings — one routing confirmed, and a single rule resolves both.**
    - **(a) `Stmt::Local` / `Arm` / field-variant-arg — routing to T13b ACCEPTED, with a corrected
      framing.** These are **noise, not corruption**: `cargo test` compiles with `cfg(test)`, so the
      mutation is real, covered by construction, and killed by its own test. The cost is a wasted compile
      and small score inflation — the same cost the human's "skip at discovery" decision avoided, at
      materially lower frequency. It produces no wrong bucket and no wrong report, so it need not
      pre-empt T13b. **But do not route it as "T13b is in `syn::Arm` anyway"** — `Stmt::Local` has nothing
      to do with the arm operator, and hitching the fix to an unrelated operator is how the other three
      quarters get forgotten. Route it as **one explicit gate-sweep item inside T13b**: `visit_stmt`,
      `visit_arm`, each a four-line gate, each a new row in
      `cfg_test_suppresses_every_item_kind_that_can_hold_a_site` (rename it
      `…every_position_that_can_hold_a_site`). The table is the enforcement; growing it is the fix.
    - **(b) Array-length literals inside types — RULING: not a `Constant` site, and the fix is one rule
      worth more than the bug.** `[u8; 1] → [u8; 0]` mutates a **type**, not behaviour: a compile-time
      constant whose mutation is a near-certain compile error, i.e. a wasted compile scored `Killed` —
      pure score inflation with zero signal. The taxonomy's `Constant` row means a `0`/`1` in *evaluated
      code*; a type has no behaviour for a test to notice.
      - **The rule: sites live in expressions, never in types. Do not descend into `syn::Type`** — a
        `visit_type` override that returns without recursing, ~4 lines in the scanner.
      - What one rule buys, disproportionately: it removes the array-length taxonomy problem entirely;
        it removes **three of the five leaks reported in (a)** — struct field, fn arg and enum variant are
        *all* `[u8; 1]`-in-type-position, not attribute leaks at all; it makes the `ForeignItem` gate's
        reachability argument near-vacuous (it should certainly stay); and it forecloses const-generic
        argument positions (`Foo<1>`, `Foo<{ 1 + 1 }>`) before they are discovered as a fourth instance of
        the same bug. Nothing of value is lost: array **repeat expressions** (`[0u8; 1]`, an `ExprRepeat`)
        are expressions and stay in scope. After that rule the residual (a) leaks are exactly
        **`Stmt::Local` and `Arm`** — two gates, not five.
      - **Does it need the human? Inform, don't block.** It is a taxonomy narrowing inside my lane, and it
        aligns code with the taxonomy as written. But it **reduces the site count** on real files, and A5
        owes the human a recorded count-divergence measurement once S6 lands — so record it as an Anders
        decision with the human's veto explicitly available, alongside `KillReason::CompileError` which is
        what will let that A5 measurement finally be made honestly. Land at T13b.
  - **6. The census finding — there is a process point, and it is not "count more carefully".** A
    conclusion right for a measurement that was wrong is an **unearned** conclusion; that it survived
    audit is luck, and luck is not a control. The specific error: a broad mechanical proxy (*how many syn
    structs carry `attrs`?*) was used to establish a narrow structural claim (*which enums must the
    visitor gate?*). The proxy can be off by 30% without perturbing the answer — exactly what happened,
    and exactly why the miss was undetectable from the conclusion.
    - **Rule to adopt: when a claim is checkable by construction or by test, do not establish it by
      census.** The true evidence for "there is no fifth item enum" is (a) the set of `visit_*_item` hooks
      syn declares, enumerable directly, and (b)
      `cfg_test_suppresses_every_item_kind_that_can_hold_a_site` with its attribute-free controls, which
      *demonstrates* the property instead of arguing for it. The round-1 `ForeignItem` FAIL was caught by
      the test, not the census — the census had already "confirmed" four enums while the fourth was
      unimplemented. That is the whole case, empirically.
    - Practical form: quantitative claims in a task write-up either (i) carry the command that produced
      them so they are reproducible, or (ii) are replaced by the test that makes them unnecessary. Prefer
      (ii). Bhaskar auditing the numbers is the right instinct and should continue — but the durable fix
      is to stop making the conclusion depend on them.
  - **7. Assessment of the T13a code itself (all endorsed, no action).**
    - **The `0f64` fix is correctly placed.** `is_integer_suffix` lives in `operators` (Core, the module
      that owns literal knowledge) and the scanner consumes it — the dependency points inward, the same
      shape as the float precondition. Gating in `constant_operator` rather than patching
      `mutate_int_literal` is right: the site should never have existed. The `0u8` control proves the
      *suffix* is the discriminator.
    - **Predicate span = `node.method.span()`** keeps the mutation a single-token replacement and is what
      lets the class free-ride the existing model; the nested/chained test pinning that each call's span
      is *its own* identifier and never the receiver's is the right proof shape.
    - **Bitwise-assign emitting no sites, pinned by its own named test** with the taxonomy cited —
      discharges the T10 carry-forward as written ("don't let a test be the only place the decision
      lives"). The scope-boundary idiom holds again: `deferred_operators_emit_no_sites` and
      `float_literals_are_not_constant_sites` were **inverted, not deleted**.
    - **`preflight_parse` / `validate_source`** is the Q4 answer implemented exactly as ruled: infra reads
      the bytes in `cli.rs`, the pure `scanner::validate_source` decides Rust-ness, one `parse` helper
      guarantees validation and discovery cannot disagree, and R1 ordering is untouched. The e2e test
      proving it by the **absence of a `target/` directory** rather than by exit code is the strongest
      available evidence and costs nothing to run.
    - **The score line** carries the denominator and the exclusion, omits the clause at zero uncovered,
      and degrades to `n/a (no mutants were run; N uncovered sites excluded)` — all three edges tested,
      singular/plural handled. It **deviates slightly from the human's illustrative wording**
      (`1 killed of 1 mutant run` vs `1 of 1 mutants run`); the shipped form is better — it names what
      the numerator *is* — but it is the human's line, so **flag for a nod, not a change**.
    - **`parse_export` into `mod tests`** — discharged exactly as ruled.
  - **Carry-forwards to T13b–T17**
    - **T13b (in order):** (1) extract `mutant_source(original, site)` — still owed from T12; (2) add
      `KillReason::CompileError` before any undecidable-precondition operator ships; (3) the
      scanner↔mapping **totality property test**; (4) the **no-sites-inside-`syn::Type`** rule; (5) the
      `Stmt`/`Arm` cfg gate as an explicit sweep item with new rows in the (renamed) position table;
      (6) the structural operators that fit the current model + the record's `canonical_token` →
      **mutation description** generalization; (7) bump `Operator::ALL.len()` deliberately.
    - **T13c (new — carved out of T13b):** `Vec<Edit>` + `splice_all` (descending start order) driven by
      **arm-body swap alone**. `replacement` survives as the single-token fast path. Blast radius
      unchanged from the T9 estimate: `site.rs`, `apply.rs` (+1 fn), one block in `pipeline.rs`.
    - **T13b/T17 — print the partial report before bailing on a mapping error;** do not convert the abort
      into a silent skip.
    - **T14/T15 — unchanged and still owed:** `function_id` uniqueness is a correctness bug before
      differential goes live; `Selection` becomes three-way (**narrow → then gate**, narrowed-out sites
      produce **no record**, no fourth bucket) — **T13a did not harden the two-way partition**, as
      required; verify `--lines` × coverage precedence against upstream source; records will want
      `function_id`; `RunConfig` is introduced at T15, not before.
    - **T16 — unchanged:** both `RestoreGuard`'s target and `crate_root` must point at the worker's
      isolated copy; coverage generation runs **once**, never per-worker.
    - **T17 — unchanged, plus one:** process-group / job-object kill (owed for two reasons); bound the
      stderr buffer; suppress mutant stdout unless `--verbose`; `check_baseline` returns the measured
      `Duration` and `timeout = baseline × --timeout-factor` (default 10); wire `--test-command`; and
      **re-measure `--mutation-warning`'s parity-inherited default 50 only after the full S6 roster
      lands** — T13a moved the count in both directions at once (+11 operators, −`0f64`-class false
      sites, and −array-length sites once the type rule lands), so any measurement before T13c is stale.
    - **`docs/design.md`** should gain the **owned-vs-foreign list rule** from item 2 under Conventions —
      a three-time pattern and the durable lesson of this slice.
  - **Nothing in T13a compromises the architecture if carried forward.** Core stays pure; the scanner's
    new `use crate::operators` is an **inward** Core→Core edge, and `is_integer_suffix` /
    `plain_decimal_float_value` are correctly homed in the module that owns literal knowledge. The only
    outward edge remains the sanctioned `pipeline → apply`/`runner`/`coverage`. `preflight_parse` reads
    the file in the adapter and delegates the decision to a pure seam — the right side of the line.
    Seams still take domain primitives; `RunConfig` correctly still does not exist. **S6 remains open.**

### A6 — upstream verification (discharged at T11, recorded verbatim)
Verified against `unclebob/mutate4go` `internal/runner/runner.go`:
- `ensureCoverage` **errors** when `--reuse-coverage` is set and the profile is absent — `LoadProfile`
  returns `nil, nil` on `os.IsNotExist`, and the caller treats that as a hard failure. Our behavior
  (error + exit `1`) therefore **matches upstream**; the provisional S4-close answer and upstream agree.
- Upstream's profile location is `target/coverage/coverage.out`. Ours is `target/coverage/coverage.json`
  — a **port of the location** to our JSON-export backend, not an invention (A2 intact).
- Upstream additionally prints to stdout: `"Reusing existing coverage; covered/uncovered classification
  may be stale."` on the reuse path. **Not implemented at T11 — owed to T12** (parity, and the user's
  only signal that the gate may be stale).

### ⚠ Product decisions owed to the human (raised at T12 close / S5 close) — RESOLVED
1. **Score-line qualifier → QUALIFY INLINE.** The score carries its denominator and the uncovered
   exclusion on the same line, e.g. `Score:     100.0% (1 of 1 mutants run; 1 site uncovered,
   excluded)`. The bare form is correct per `docs/design.md` but invites a "fully tested" misread on a
   partially-covered file. Lands in **T13** (cheap rendering change; `summary()` is already pure
   rendering over the records). The summary remains a **change-detector, not a stability contract**
   pre-1.0 — D4 serializes records and must never re-parse `summary()`.
2. **Mutant stdout → SUPPRESS BY DEFAULT, SURFACE UNDER `--verbose` (T17).** Confirmed. Today the
   runner inherits child stdout, so a real run prints one full `cargo test` transcript per mutant and
   buries the report; the e2e test had to count uncovered *entries* rather than grep a line number
   because of it. The `--verbose` flag is already reserved for exactly this. Accepted as a UX
   divergence that changes what a CI log looks like. The baseline's stderr capture stays as-is — it
   is what makes the red-baseline abort diagnosable.
3. **`--lines` × coverage precedence → CONFIRMED: FILTERS NARROW, GATING CLASSIFIES.** A covered site
   outside `--lines` is **not** `Uncovered` — recording it as such would be a false statement about the
   test suite and would corrupt the one bucket S5 exists to produce. `MutantOutcome` must **not** grow
   a fourth bucket (A8/parity). A narrowed-out site produces **no record at all**; coverage gating
   then partitions only what survived narrowing. The summary may carry a non-bucket line
   (`Sites skipped by --lines: N`). Land the three-way shape at **T15**, when the second filter
   actually arrives (YAGNI) — but **T13 must not harden the current two-way partition into an
   assumption**. Upstream's own `--lines`×coverage precedence is still **owed verification at T15**
   (same discipline as the A6 resolution: check the source, record what it does).

### ⚠ Product decisions owed to the human (raised at T11 close) — RESOLVED
1. **Sites inside the target file's own `#[cfg(test)] mod tests` → SKIP AT DISCOVERY, IN T12.**
   Anders' option (a). The scanner gains `cfg(test)` filtering so test-module items are never
   discovered as sites. Rationale: test-module lines are *always* covered by construction, so
   covered-only gating would preferentially mutate test code while skipping genuinely uncovered
   production code; a mutant of an assertion constant is killed by its own test, costs a full compile,
   and carries no signal about test quality. This is a **deliberate divergence from upstream
   mutate4go**, which never faced it (Go tests live in a separate `_test.go` file the one-file-at-a-time
   model never selects). Lands in **T12 alongside covered-only gating**; expect the site count to drop.
2. **`cargo-llvm-cov` install in CI → KEEP THE ACTION, PINNED TO A COMMIT SHA.** `taiki-e/install-action`
   stays (it is materially faster than `cargo install cargo-llvm-cov --locked` at ~5.5 min/leg/run), but
   the **moving `@v2` major tag is replaced with a full commit SHA** so the supply-chain surface is
   frozen. T12 must make the change and record the pinned SHA. Whether the Windows leg resolves a
   prebuilt remains to be observed on the first green run.
3. **Visibility split → CONFIRMED.** `coverage` / `coverage_map` and their items tighten to
   `pub(crate)` **in T12**, at the commit that first gives them real callers (tightening earlier would
   trip `-D warnings` as `dead_code`). The **pre-existing** accidental surface (`apply`, `runner`,
   `scanner`, `site`, `manifest`, `outcome`) stays with the **S8** sweep. Recorded as a T12 acceptance
   item so it is not lost between the two.
4. **`#![forbid(unsafe_code)]` → ADD IN T12.** The project profile has always mandated it and
   `src/lib.rs` has never carried it. One line; stop carrying an unmet stated convention.

### ⚠ S5 slice-level items owed to the human (raised at S4 close) — RESOLVED
1. **R8 / external tool dependency — CONFIRMED.** `cargo-llvm-cov` + `llvm-tools-preview` go into
   **both** CI legs (`ubuntu-latest` **and** `windows-latest`) at **T11**, and a missing tool is a hard
   exit-`1` carrying an install hint — never a silent mutate-everything (A6). The raised contributor
   onboarding cost is accepted.
2. **A6 open edge — VERIFY, DON'T GUESS.** T11 must resolve `--reuse-coverage`-without-coverage against
   **upstream `unclebob/mutate4go` source** and then **match upstream's behavior**. The provisional
   "error and tell the user" answer is a fallback only if upstream is genuinely silent on the case;
   record what upstream actually does either way.
3. **R3/D6 panic-killed mutants — OPTION (b).** T12's per-mutant records make panic-kills
   **distinguishable in the record while the bucket stays `Killed`** (A8 unchanged). D6 stays deferred;
   the records tell us whether it is worth taking.
4. **Visibility item noted** — record the expected count-divergence magnitude once S6 lands. A5 stands
   as written.

### S3 slice-level assumptions — awaiting human sign-off
- **S3 mutates the user's real source file in place.** The only crash backstop is VCS (T7's documented
  `SIGKILL`/OOM caveat); nothing today refuses to run on a dirty target. Accepted for MVP — confirm
  knowingly.
- **`Uncovered` is a structurally-present, always-zero bucket until S5.** The slice's
  "killed/survived/uncovered report" is satisfied in shape, not yet in production of the third bucket.
- **Baseline abort is now a hard precondition:** a red or hanging suite fails the whole run with exit
  `1` — correct, and **stricter than upstream**; it changes what a CI invocation does on an
  already-broken build.

### ⚠ Product decisions owed to the human (raised at S3 close) — RESOLVED
1. **Does the report name surviving mutants? → RECORDS, AT T12.** `MutationReport` grows per-mutant
   records (`file:line` + operator/kind + outcome) at **T12/S5**, where uncovered-site listing (A6)
   forces the same shape anyway; counters become **derived** and `summary()` becomes a rendering of the
   model. Not retrofitted now — T9's counter shape stands until T12.
2. **Are float constants (`0.0↔1.0`) in scope? → IN SCOPE, BUT NOT T10.** Parked in **S6** with the other
   idiomatic operators, NOT added to S4's arithmetic completions. T10 therefore stays integer-only.
   Requires a `syn::LitFloat` scanner arm + a float-aware constant mapping preserving `f32`/`f64`
   suffixes (`mutate_int_literal` will not stretch — separate path).

### S3 slice-level assumptions — SIGNED OFF by the human at S3 closeAll three accepted as stated: (1) in-place mutation of the real source with VCS as the only crash
backstop, and **no dirty-target guard** (explicitly declined as a task — revisit only if it bites);
(2) `Uncovered` structurally present but always zero until S5; (3) baseline abort as a hard
precondition (exit `1` on a red/hanging suite), knowingly **stricter than upstream mutate4go**.

### Product decision — exit codes (C11) — RESOLVED: strict mutate4go parity
**Human decision:** strict parity. Exit `0` = ran OK **including surviving mutants**; exit `1` = **any
error** (usage OR operational). mutate4rust does **NOT** fail CI on survivors — survivors are reported,
not signalled via exit code — matching mutate4go exactly. Implications:
- The T2 stub contract (`0/1/2/3`) is **superseded**: collapse usage+operational → `1`; survivors do
  not affect the exit code. clap's default usage exit (2) is overridden to `1` for parity.
- Corrected in `src/cli.rs` alongside T3 (S1 close); `docs/design.md` (T3) documents the parity
  contract.
- Guardrail note: users wanting CI-fail-on-survivors read the reported survivor count; a future
  opt-in `--fail-on-survivors` flag is a candidate deferral (not now).
