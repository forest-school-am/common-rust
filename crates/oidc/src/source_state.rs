//! Whether the sources feeding this binary were committed when it was built,
//! and what that costs a prod boot. The build-time DETECTION lives in
//! build.rs; anything about how a template is served belongs in web.rs.

use common_logging::Deployment;

/// The vocabulary build.rs and this crate share. build.rs emits a `const` of
/// this type, so the generated code is TYPE-CHECKED against this declaration:
/// renaming a variant or changing its payload breaks the build instead of
/// silently disabling the refusal below. It is deliberately not a string —
/// three literals matched across a compilation boundary is exactly the drift
/// CODESTYLE 4.5 exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceState {
    Clean,
    /// Count of uncommitted artifact-feeding files at build time.
    Dirty(usize),
    /// git could not be consulted. NOT an assurance of either other state.
    Unknown,
}

pub(crate) fn dirty_source_refusal(deployment: Deployment, state: SourceState) -> Option<String> {
    match (deployment, state) {
        (Deployment::Prod, SourceState::Dirty(n)) => Some(format!(
            "common-oidc was built from a dirty working tree ({n} uncommitted \
             file(s) under src/, templates/ or Cargo.toml); the served shim is \
             unreproducible and this is refused under DEPLOYMENT_TYPE=prod. \
             Commit the crate, or build from a clean checkout."
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_refuses_a_dirty_crate_source_and_dev_does_not() {
        assert!(dirty_source_refusal(Deployment::Prod, SourceState::Dirty(3)).is_some());
        assert!(dirty_source_refusal(Deployment::Dev, SourceState::Dirty(3)).is_none());
    }

    #[test]
    fn clean_and_unknown_both_boot_but_mean_different_things() {
        assert!(dirty_source_refusal(Deployment::Prod, SourceState::Clean).is_none());
        assert!(dirty_source_refusal(Deployment::Prod, SourceState::Unknown).is_none());
    }

    /// The count reaches the operator: a refusal that will not say HOW dirty
    /// sends them looking with no idea what they are looking for.
    #[test]
    fn the_refusal_reports_how_many_files_are_uncommitted() {
        let msg = dirty_source_refusal(Deployment::Prod, SourceState::Dirty(7)).expect("refusal");
        assert!(msg.contains('7'), "refusal must name the count: {msg}");
    }
}
