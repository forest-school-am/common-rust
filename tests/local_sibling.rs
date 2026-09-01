#[test]
fn sees_local_common_logging() {
    // Deployment::parse and as_str exist only in the LOCAL common-logging
    // (v0.1.1..HEAD), not in the published v0.1.1 this crate used to build
    // against. If the patch is inert, this will not compile.
    assert_eq!(common_logging::Deployment::parse(Some("prod")).unwrap().as_str(), "prod");
    assert!(common_logging::Deployment::parse(Some("Prod")).is_err());
}
