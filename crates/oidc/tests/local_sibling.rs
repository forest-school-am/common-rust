#[test]
fn sees_local_common_logging() {
    assert_eq!(common_logging::Deployment::parse(Some("prod")).unwrap().as_ref(), "prod");
    assert!(common_logging::Deployment::parse(Some("Prod")).is_err());
}
