//! `mutate4rust` core library.
//!
//! This crate hosts the mutation-testing engine. The [`site`], [`scanner`],
//! `operators`, and [`outcome`] modules are the pure **Core/domain** layer (the
//! mutation-site model, the `syn` site-discovery walk, the operator mappings, and
//! the result buckets — no fs/process/argv/clap); `pipeline` and `report` are
//! the **application** layer (the mutate loop and the Killed/Survived/Uncovered
//! reporter); [`cli`], [`manifest`], [`apply`], and [`runner`] are the outer
//! infrastructure adapters (argv parsing, TOML sidecar fs I/O, mutant
//! apply/restore, and the test-command subprocess). Coverage gating lands in a
//! later slice.

pub mod apply;
pub mod cli;
pub mod manifest;
mod operators;
pub mod outcome;
mod pipeline;
mod report;
pub mod runner;
pub mod scanner;
pub mod site;

/// Returns the crate's package version, as declared in `Cargo.toml`.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn version_matches_cargo_manifest() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
