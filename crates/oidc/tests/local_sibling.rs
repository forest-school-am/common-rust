//! That common-logging resolved to THIS workspace and not to something else,
//! asserted from a sibling crate (R22a). Anything about OIDC's own behaviour
//! belongs in mock_flow.rs.

#[test]
fn sees_local_common_logging() {
    assert_eq!(
        common_logging::Deployment::parse(Some("prod"))
            .unwrap()
            .as_ref(),
        "prod"
    );
    assert!(common_logging::Deployment::parse(Some("Prod")).is_err());
}
