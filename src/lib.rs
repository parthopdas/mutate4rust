//! `mutate4rust` core library.
//!
//! This crate hosts the mutation-testing engine. During the S1 bootstrap it only
//! exposes a trivial helper so the build/test gates have real code to exercise;
//! the discovery, mutation, and reporting pipeline lands in later slices.

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
