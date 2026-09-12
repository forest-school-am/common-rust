//! A consuming service in miniature, for the spawned-process tests: valid
//! logging config, `init()`, an ordinary `startup` event, then a refusal from
//! its own module. Nothing about the crate's own behaviour belongs here —
//! that is `src/`'s tests.

mod boot {
    pub fn refuse_bind() -> ! {
        common_logging::refuse!(common_logging::Refusal::new(
            "PROBE_BIND",
            "not-an-address",
            "a socket address such as \"0.0.0.0:8080\"",
        ))
    }
}

fn main() -> ! {
    common_logging::init();
    common_logging::info!(common_logging::STARTUP, "ordinary startup event");
    boot::refuse_bind()
}
