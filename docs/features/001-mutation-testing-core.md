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
| T10 | S4 | Arithmetic idiomatic completions: `/→*`, `%→*`, compound-assignment ops. | Pending | - |
| T11 | S5 | `cargo-llvm-cov` invocation + profile parse; region→line coverage map. | Pending | - |
| T12 | S5 | Covered-only gating; uncovered sites reported & skipped; `--reuse-coverage`; coverage-absent behavior (A6). | Pending | - |
| T13 | S6 | Rust-specific operators: `Option`/`Result`, `match`-arm, `unwrap`/`expect`, `?`, bitwise; precondition-gated emission (A8-adjacent). | Pending | - |
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

### S3 slice-level assumptions — awaiting human sign-off
- **S3 mutates the user's real source file in place.** The only crash backstop is VCS (T7's documented
  `SIGKILL`/OOM caveat); nothing today refuses to run on a dirty target. Accepted for MVP — confirm
  knowingly.
- **`Uncovered` is a structurally-present, always-zero bucket until S5.** The slice's
  "killed/survived/uncovered report" is satisfied in shape, not yet in production of the third bucket.
- **Baseline abort is now a hard precondition:** a red or hanging suite fails the whole run with exit
  `1` — correct, and **stricter than upstream**; it changes what a CI invocation does on an
  already-broken build.

### ⚠ Product decisions owed to the human (raised at S3 close)
1. **Does the report name surviving mutants?** Counts only (current, `design.md`-faithful, minimal) vs.
   per-mutant records with `file:line` + operator (upstream-like, actionable, and what S5's uncovered
   listing needs anyway). Anders recommends **records, landed at T12** — decide before S5, since it
   changes the reporter's shape.
2. **Are float constants (`0.0↔1.0`) in scope?** Add to S4's idiomatic completions, or record a
   permanent `Dx` deferral. Today it is an unstated gap — the taxonomy never mentions floats.

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
