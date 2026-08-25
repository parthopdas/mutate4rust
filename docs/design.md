# Design

High-level design & architecture for **mutate4rust**. Every agent reloads this file before acting
(golden rule #1); keep it crisp and current. Feature SSOT: `docs/features/001-mutation-testing-core.md`.

## System overview

**mutate4rust** is a Rust port of `unclebob/mutate4go`: a single-file mutation-testing CLI used as a
guardrail in Rust agentic development. It operates on **one `.rs` file at a time** and runs a fixed
pipeline:

**discover → coverage → mutate → test → report → manifest**

Sites are discovered with `syn`, gated by `cargo-llvm-cov` coverage (uncovered sites are reported, not
executed), mutated via byte-span splices, and each mutant is validated by running the crate's tests.
Outcomes are bucketed **Killed / Survived / Uncovered**. A committed **TOML sidecar manifest**
(`<file>.rs.m4r.toml`) records last-run date + per-function hashes to drive **differential** runs
("changed functions only") on subsequent invocations. The CLI/UX surface mirrors mutate4go 1:1 for
muscle-memory parity; the operator set adds Rust-idiomatic mutations active by default.

## Architecture

**Layered, with a strict inward dependency flow** (Clean Architecture). Dependency arrows point
**inward**; outer layers depend on inner, never the reverse.

- **Core / domain** (pure — no fs, process, argv, or clap): the mutation-site model
  (`kind`, byte-span, line, function id) and operator mappings. `operators`/the site model must
  **never** `use` the runner, coverage, or fs. Keep it deterministic and side-effect-free.
- **Application / pipeline**: orchestrates discover → coverage → mutate → test → report → manifest
  (the post-run manifest update is a real stage, S7/T15). Depends on
  core; reaches infrastructure only through narrow seams named at the boundary.
- **Infrastructure / adapters** (the **only** layer touching process/fs/argv): the `cargo test`
  runner, `cargo-llvm-cov` coverage, TOML sidecar I/O, and the clap CLI.

**Rules & boundaries:**
- No premature trait seams (YAGNI) — just name the boundary so downstream tasks land on the right side.
- All modules live **inside the lib**; `main.rs` is a thin outer adapter that only forwards the process
  exit code. No workspace/multi-crate split at MVP.

**CLI → config → pipeline seam.** The clap-derive `Cli` is an **adapter type** translated at the edge
into a validated, **clap-free `RunConfig`** (mirroring upstream `cli.Options`). The boundary applies
defaults and parses raw flags into domain types — e.g. `lines: Option<String>` → `BTreeSet<usize>`.
Run modes are modelled as a **`Mode` enum** (`Scan` | `UpdateManifest` | `Mutate`) resolved at that
same boundary, which enforces their mutual exclusivity. *Latent gap (to close in a later slice):*
`run()` currently routes `--update-manifest` into the mutation stub rather than a distinct mode.

## Key components

- **CLI** — clap adapter: parses argv, builds `RunConfig`/`Mode`; `--help`/`--version`.
- **Scanner** — `syn`/`proc-macro2` site discovery over byte spans.
- **Operators** — parity + idiomatic taxonomy; pure mappings from a site to mutated bytes.
- **Writer** — byte-span apply/restore (splice over original bytes; never `syn`-reprint the file).
- **Runner** — `cargo test` execution with per-mutant timeout.
- **Coverage** — `cargo-llvm-cov` invocation + region→line mapping.
- **Manifest / differential** — TOML sidecar read/write; per-function hashing; changed-function
  selection.
- **Reporter** — Killed / Survived / Uncovered buckets and counts.

## Cross-cutting concerns

- **Error handling:** `anyhow` at the adapter/application edge; introduce typed errors in core only if
  a later slice needs them. Fail fast with clear messages (e.g. unparsable target file).
- **Config & secrets:** no secrets or hardcoded config; nothing to inject at MVP.
- **Exit-code contract (strict mutate4go parity — decision C11):**
  - `0` = ran OK, **including when mutants survive** (survivors are reported, not signalled by exit
    code — mutate4rust does **not** fail CI on survivors).
  - `1` = **any error** — usage errors and operational errors both map to `1` (upstream
    `runner.StatusCode` returns 1 on any error and does not distinguish the two). clap's default
    usage-error exit (2) is overridden to `1`.
  - A future opt-in `--fail-on-survivors` flag is a candidate deferral, not now.
  - Forward-pointer: future CLI mutual-exclusivity violations (S2/S7/S8 via clap
    `conflicts_with`/`ArgGroup`) also resolve to exit **1** under this contract — land that work on
    the parity code path, not clap's default `2`.
- **Determinism:** discovery, hashing, and reporting must be deterministic (stable ordering, normalized
  hashes) so differential runs don't churn.

## Conventions

- **Stack:** Rust **2024 edition**; modules inside the lib, thin `main.rs`.
- **Fast gates (must pass clean, zero warnings):** `cargo build`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test`.
- **Line endings:** LF, enforced via `.gitattributes` (`* text=auto eol=lf`).
- **Parity-vs-idiomatic principle:** parity for the CLI/UX surface and the arithmetic parity mappings
  (`+→-`, `-→+`, `*→/`); Rust-idiomatic operators are additive and **active by default** (default runs
  are intentionally **not** count-parity with mutate4go — accepted tradeoff A5).
- **Report parity:** the **buckets** are 1:1 parity with mutate4go — Killed / Survived / Uncovered
  (timeout and non-compiling mutants folded into Killed); the numeric **score**
  (`killed / (killed + survived)`, `uncovered` excluded and reported separately) is an **additive
  mutate4rust extension** — upstream emits raw counts, no numeric score — so "parity" here means bucket
  parity, not full output parity.
- **Least-privilege visibility:** prefer the narrowest access modifier that works.
- **Sites live in expressions, never in types.** The scanner does not descend into `syn::Type`. A
  literal in type position (an array length, a const-generic argument) is a compile-time constant whose
  mutation is a near-certain compile error — a wasted build scored `Killed`, i.e. score inflation with
  zero signal. Array **repeat expressions** (`[0u8; 1]`) are expressions and stay in scope.
- **Hand-kept lists — classify by who owns the domain.** A list that must stay in sync with something
  else is a silent hole; the remedy depends on ownership.
  - **We own the domain** (e.g. `Operator::ALL`) ⇒ make the omission **inexpressible**. Generate the
    list and its consumers from a single declaration (`declare_operators!`). Enforcement by
    construction; a second definition is a compile error.
  - **A foreign `#[non_exhaustive]` domain** (e.g. `syn::Item`, `ForeignItem`, `TraitItem`, `ImplItem`)
    ⇒ enforcement is impossible by construction *and* by compiler, so: (a) choose the wildcard default
    whose failure mode you can live with, and **say which** in the code; (b) enumerate the known
    variants explicitly anyway, so the audited set is readable in one place; (c) **make the test the
    enforcement** — a fixture table pairing each construct with an attribute-free control. The test,
    not the match, holds the property, and it is the artifact a dependency bump must be re-run against.
  - Corollary: a comment that **enumerates its own holes** is worth more than one promising coverage it
    cannot deliver. Never claim universal completeness over a foreign domain.
- **Do not establish by census what is checkable by construction or by test.** A conclusion that is
  right for a measurement that was wrong is unearned. Quantitative claims in a write-up either carry
  the command that produced them, or — preferably — are replaced by the test that makes them
  unnecessary.
