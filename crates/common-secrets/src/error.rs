//! The failure taxonomy. Fail-closed: every path that does not return a value
//! returns one of these — there is no empty-secret success. Messages carry the
//! stage and status only, never a credential or a fetched value.

/// Which leg of the login->login->read chain raised the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    AuthentikToken,
    VaultLogin,
    VaultRead,
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Stage::AuthentikToken => "authentik token request",
            Stage::VaultLogin => "OpenBao jwt login",
            Stage::VaultRead => "OpenBao kv read",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("secret client misconfigured: {0}")]
    Config(String),
    #[error("network failure during {stage}: {detail}")]
    Network { stage: Stage, detail: String },
    #[error("credentials rejected during {0}")]
    AuthRejected(Stage),
    #[error("secret not found")]
    NotFound,
    #[error("upstream anomaly during {stage}: {detail}")]
    Upstream { stage: Stage, detail: String },
}
