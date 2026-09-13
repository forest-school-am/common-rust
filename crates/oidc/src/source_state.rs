//! Whether the sources feeding this binary were committed when it was built,
//! and what that costs a prod boot. The build-time DETECTION lives in
//! build.rs; anything about how a template is served belongs in web.rs.

use common_logging::Deployment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceState {
    Clean,
    Dirty { uncommitted: usize },
    Unknown,
}

pub(crate) fn dirty_source_refusal(deployment: Deployment, state: SourceState) -> Option<String> {
    match (deployment, state) {
        (Deployment::Prod, SourceState::Dirty { uncommitted: n }) => Some(format!(
            "common-oidc was built from a dirty working tree ({n} uncommitted \
             file(s) under {}); the served shim is unreproducible and this is \
             refused under DEPLOYMENT_TYPE=prod. Commit the crate, or build \
             from a clean checkout.",
            crate::ARTIFACT_SOURCES.join(", ")
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_refuses_a_dirty_crate_source_and_dev_does_not() {
        assert!(
            dirty_source_refusal(Deployment::Prod, SourceState::Dirty { uncommitted: 3 }).is_some()
        );
        assert!(
            dirty_source_refusal(Deployment::Dev, SourceState::Dirty { uncommitted: 3 }).is_none()
        );
    }

    #[test]
    fn clean_and_unknown_both_boot_but_mean_different_things() {
        assert!(dirty_source_refusal(Deployment::Prod, SourceState::Clean).is_none());
        assert!(dirty_source_refusal(Deployment::Prod, SourceState::Unknown).is_none());
    }

    #[test]
    fn the_refusal_reports_how_many_files_are_uncommitted() {
        let msg = dirty_source_refusal(Deployment::Prod, SourceState::Dirty { uncommitted: 7 })
            .expect("refusal");
        assert!(msg.contains('7'), "refusal must name the count: {msg}");
    }

    #[test]
    fn the_refusal_names_every_path_that_was_actually_checked() {
        let msg = dirty_source_refusal(Deployment::Prod, SourceState::Dirty { uncommitted: 1 })
            .expect("refusal");
        assert!(
            msg.contains("src, Cargo.toml, build.rs"),
            "refusal must name exactly the checked paths: {msg}"
        );
    }
}
