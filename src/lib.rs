//! `mutate4rust` core library.
//!
//! This crate hosts the mutation-testing engine. The [`site`] and [`scanner`]
//! modules are the pure **Core/domain** layer (the mutation-site model and the
//! `syn` site-discovery walk — no fs/process/argv/clap); [`cli`] and [`manifest`]
//! are the outer infrastructure adapters (argv parsing, TOML sidecar fs I/O).
//! Coverage, mutation, and reporting land in later slices.

pub mod cli;
pub mod manifest;
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
