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
| T12 | S5 | Covered-only gating; uncovered sites reported & skipped; `--reuse-coverage`; coverage-absent behavior (A6); **report gains per-mutant records (counters become derived)**. | Pending | - |
| T13 | S6 | Rust-specific operators: `Option`/`Result`, `match`-arm, `unwrap`/`expect`, `?`, bitwise, **float constants (`0.0↔1.0`)**; precondition-gated emission (A8-adjacent). | Pending | - |
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
