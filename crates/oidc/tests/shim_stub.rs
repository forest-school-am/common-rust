//! The test stub and the served shim must carry the SAME transport. They are
//! two files because only one of them may install a guard at load; the code the
//! generated client actually calls is identical in both, and this is what keeps
//! it that way.

const SHIM: &str = include_str!("../src/common-oidc.ts");
const STUB: &str = include_str!("../src/common-oidc.test-stub.ts");

fn transport<'a>(src: &'a str, which: &str) -> &'a str {
    let start = src
        .find("// >>> transport")
        .unwrap_or_else(|| panic!("{which}: the transport start marker is gone"));
    let end = src
        .find("// <<< transport")
        .unwrap_or_else(|| panic!("{which}: the transport end marker is gone"));
    assert!(start < end, "{which}: the transport markers are inverted");
    &src[start..end]
}

#[test]
fn the_stub_carries_the_shims_transport_unchanged() {
    assert_eq!(
        transport(SHIM, "shim"),
        transport(STUB, "stub"),
        "the stub's transport has drifted from the shim's — the marked block is \
         copied across verbatim, so bring the two back into agreement"
    );
}

#[test]
fn the_stub_does_not_install_the_guard_at_load() {
    // The shim's whole reason for having a stub: this bare call at module load.
    assert!(
        SHIM.contains("\ninstallReauthGuard();"),
        "the shim no longer installs its guard at load — if that is deliberate, \
         the stub may no longer be needed at all"
    );
    assert!(
        !STUB.contains("\ninstallReauthGuard();"),
        "the stub must not run the guard at load: it would wrap the fetch the \
         test installed, and jsdom refuses the navigation it can reach"
    );
}

#[test]
fn both_expose_the_surface_the_generated_client_imports() {
    for (name, src) in [("shim", SHIM), ("stub", STUB)] {
        for wanted in [
            "export function installReauthGuard",
            "export class CallFailure",
            "export async function call",
        ] {
            assert!(src.contains(wanted), "{name} is missing `{wanted}`");
        }
    }
}
