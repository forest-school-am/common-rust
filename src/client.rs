use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreErrorResponseType, CoreGenderClaim,
    CoreJsonWebKey, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreRevocableToken,
    CoreRevocationErrorResponse, CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::{
    AccessToken, AdditionalClaims, AuthUrl, AuthorizationCode, Client, ClientId, CsrfToken,
    EmptyExtraTokenFields, EndpointNotSet, EndpointSet, IdTokenFields, IssuerUrl, JsonWebKeySet,
    Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken,
    Scope, StandardErrorResponse, StandardTokenResponse, TokenUrl, UserInfoClaims, UserInfoUrl,
};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::OidcConfig;
use crate::error::Error;
use crate::principal::Principal;

/// The stand's custom claim, served by the shared `effective_groups` scope
/// mapping (downward closure of group UUIDs).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StandClaims {
    // kept as strings so Principal::from_userinfo is the single UUID-parsing
    // / fail-closed point shared with the bearer validator
    #[serde(default)]
    effective_groups: Vec<String>,
}
impl AdditionalClaims for StandClaims {}

/// `CoreTokenResponse` with our additional claims (the client's token
/// response type must match its claims type).
type StandTokenResponse = StandardTokenResponse<
    IdTokenFields<
        StandClaims,
        EmptyExtraTokenFields,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    >,
    CoreTokenType,
>;

/// A `CoreClient` carrying our additional claims (the claims type of
/// `user_info` responses is fixed by the client type), with the three
/// endpoints we use statically set.
type StandCore = Client<
    StandClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    StandTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,    // auth
    EndpointNotSet, // device auth
    EndpointNotSet, // introspection
    EndpointNotSet, // revocation
    EndpointSet,    // token
    EndpointSet,    // userinfo
>;

/// Tokens as returned by an exchange or refresh.
#[derive(Debug, Clone)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

/// Discovery-configured OIDC protocol client. All hand-shaking goes through
/// the `openidconnect` crate; this wrapper only pins the stand conventions
/// (issuer/backchannel origin split, PKCE public client, userinfo → Principal).
pub struct StandClient {
    core: StandCore,
    http: reqwest::Client,
    config: OidcConfig,
}

impl StandClient {
    /// Fetch the provider's discovery document and build the client.
    ///
    /// Origin policy: the document is fetched via the backchannel origin;
    /// the authorize endpoint is normalized onto the browser-canonical
    /// `issuer` origin, token/userinfo onto the backchannel origin
    /// (authentik builds these URLs from the request host, so a doc fetched
    /// via the backchannel carries backchannel hosts throughout).
    pub async fn discover(config: OidcConfig) -> Result<Self, Error> {
        let http = reqwest::Client::builder()
            // openidconnect requirement: no auto-redirects on token calls
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(config.danger_accept_invalid_certs)
            .build()
            .map_err(|e| Error::Config(format!("http client: {e}")))?;

        let back = config.backchannel.clone().unwrap_or_else(|| config.issuer.clone());
        let disco_url = {
            let base = OidcConfig::swap_origin(&config.issuer, &back);
            let mut s = base.to_string();
            if !s.ends_with('/') {
                s.push('/');
            }
            s + ".well-known/openid-configuration"
        };
        let doc: serde_json::Value = http
            .get(&disco_url)
            .send()
            .await
            .map_err(|e| Error::Discovery(format!("GET {disco_url}: {e}")))?
            .error_for_status()
            .map_err(|e| Error::Discovery(format!("GET {disco_url}: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Discovery(format!("parse {disco_url}: {e}")))?;

        let endpoint = |key: &str, base: &Url| -> Result<Url, Error> {
            let raw = doc
                .get(key)
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Discovery(format!("discovery document lacks {key}")))?;
            let u = Url::parse(raw).map_err(|e| Error::Discovery(format!("{key}: {e}")))?;
            Ok(OidcConfig::swap_origin(&u, base))
        };
        let auth_url = endpoint("authorization_endpoint", &config.issuer)?;
        let token_url = endpoint("token_endpoint", &back)?;
        let userinfo_url = endpoint("userinfo_endpoint", &back)?;

        // ID tokens are never verified (identity comes from userinfo, per
        // request) — issuer and jwks are only structural here.
        let core: StandCore = Client::new(
            ClientId::new(config.client_id.clone()),
            IssuerUrl::from_url(config.issuer.clone()),
            JsonWebKeySet::new(vec![]),
        )
        .set_auth_uri(AuthUrl::from_url(auth_url))
        .set_token_uri(TokenUrl::from_url(token_url))
        .set_user_info_url(UserInfoUrl::from_url(userinfo_url))
        .set_redirect_uri(RedirectUrl::from_url(config.redirect_url.clone()));

        Ok(Self { core, http, config })
    }

    pub fn config(&self) -> &OidcConfig {
        &self.config
    }

    /// Browser authorize URL. `silent` adds `prompt=none` (invisible while
    /// the SSO session is alive; comes back `error=login_required` if not).
    /// Returns (url, state, pkce_verifier) — persist state and verifier in
    /// the short-lived flow cookie.
    pub fn authorize_url(&self, silent: bool) -> (Url, String, String) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut req = self
            .core
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);
        for s in &self.config.scopes {
            if s != "openid" {
                // authorize_url adds the openid scope itself
                req = req.add_scope(Scope::new(s.clone()));
            }
        }
        if silent {
            req = req.add_prompt(CoreAuthPrompt::None);
        }
        let (url, state, _nonce) = req.url();
        (url, state.secret().clone(), verifier.secret().clone())
    }

    /// Redeem the authorization code (PKCE).
    pub async fn exchange_code(&self, code: String, verifier: String) -> Result<TokenBundle, Error> {
        let resp = self
            .core
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&self.http)
            .await
            .map_err(|e| Error::Exchange(e.to_string()))?;
        Ok(TokenBundle {
            access_token: resp.access_token().secret().clone(),
            refresh_token: resp.refresh_token().map(|t| t.secret().clone()),
        })
    }

    /// Server-side refresh (v5: the refresh token never reaches a browser).
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, Error> {
        let rt = RefreshToken::new(refresh_token.to_owned());
        let resp = self
            .core
            .exchange_refresh_token(&rt)
            .request_async(&self.http)
            .await
            .map_err(|e| Error::Refresh(e.to_string()))?;
        Ok(TokenBundle {
            access_token: resp.access_token().secret().clone(),
            // authentik rotates refresh tokens; keep the old one if the
            // response omits a new one
            refresh_token: resp
                .refresh_token()
                .map(|t| t.secret().clone())
                .or_else(|| Some(refresh_token.to_owned())),
        })
    }

    /// Per-request validation: ask authentik's userinfo who this access
    /// token is; fail closed on anything but a 200. This is the v4 rule —
    /// and the whole integration for bearer-API services (the mint pattern),
    /// which skip the BFF session machinery entirely.
    pub async fn principal_from_access_token(&self, access_token: &str) -> Result<Principal, Error> {
        let claims: UserInfoClaims<StandClaims, CoreGenderClaim> = self
            .core
            .user_info(AccessToken::new(access_token.to_owned()), None)
            .request_async(&self.http)
            .await
            .map_err(|e| Error::Userinfo(e.to_string()))?;

        Principal::from_userinfo(
            claims.subject().as_str(),
            claims.preferred_username().map(|u| u.as_str().to_owned()),
            claims.email().map(|e| e.as_str().to_owned()),
            &claims.additional_claims().effective_groups,
        )
        .map_err(Error::Userinfo)
    }
}
