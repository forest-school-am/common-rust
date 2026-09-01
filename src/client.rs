//! OIDC protocol only: discovery, the authorize URL, code exchange, refresh,
//! userinfo. Speaks to the IdP and knows nothing about HTTP handlers,
//! cookies or sessions — those belong in web.rs.

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
use crate::error::OidcError;
use crate::principal::Principal;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OidcClaims {
    #[serde(default)]
    effective_groups: Vec<String>,
}
impl AdditionalClaims for OidcClaims {}

type OidcTokenResponse = StandardTokenResponse<
    IdTokenFields<
        OidcClaims,
        EmptyExtraTokenFields,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    >,
    CoreTokenType,
>;

type OidcCore = Client<
    OidcClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    OidcTokenResponse,
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

#[derive(Debug, Clone)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

pub struct OidcClient {
    core: OidcCore,
    http: reqwest::Client,
    config: OidcConfig,
}

pub struct AuthorizeRequest {
    pub url: Url,
    pub csrf_state: String,
    pub pkce_verifier: String,
}

impl OidcClient {
    pub async fn discover(config: OidcConfig) -> Result<Self, OidcError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(config.danger_accept_invalid_certs)
            .build()
            .map_err(|e| OidcError::Config(format!("http client: {e}")))?;

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
            .map_err(|e| OidcError::Discovery(format!("GET {disco_url}: {e}")))?
            .error_for_status()
            .map_err(|e| OidcError::Discovery(format!("GET {disco_url}: {e}")))?
            .json()
            .await
            .map_err(|e| OidcError::Discovery(format!("parse {disco_url}: {e}")))?;

        let endpoint = |key: &str, base: &Url| -> Result<Url, OidcError> {
            let raw = doc
                .get(key)
                .and_then(|v| v.as_str())
                .ok_or_else(|| OidcError::Discovery(format!("discovery document lacks {key}")))?;
            let u = Url::parse(raw).map_err(|e| OidcError::Discovery(format!("{key}: {e}")))?;
            Ok(OidcConfig::swap_origin(&u, base))
        };
        let auth_url = endpoint("authorization_endpoint", &config.issuer)?;
        let token_url = endpoint("token_endpoint", &back)?;
        let userinfo_url = endpoint("userinfo_endpoint", &back)?;

        // ID tokens are never verified (identity comes from userinfo, per
        // request) — issuer and jwks are only structural here.
        let core: OidcCore = Client::new(
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

    pub fn authorize_url(&self, silent: bool) -> AuthorizeRequest {
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
                req = req.add_scope(Scope::new(s.clone()));
            }
        }
        if silent {
            req = req.add_prompt(CoreAuthPrompt::None);
        }
        let (url, state, _nonce) = req.url();
        AuthorizeRequest {
            url,
            csrf_state: state.secret().clone(),
            pkce_verifier: verifier.secret().clone(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn exchange_code(&self, code: String, verifier: String) -> Result<TokenBundle, OidcError> {
        let resp = self
            .core
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(verifier))
            .request_async(&self.http)
            .await
            .map_err(|e| OidcError::Exchange(e.to_string()))?;
        Ok(TokenBundle {
            access_token: resp.access_token().secret().clone(),
            refresh_token: resp.refresh_token().map(|t| t.secret().clone()),
        })
    }

    #[tracing::instrument(skip_all)]
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, OidcError> {
        let rt = RefreshToken::new(refresh_token.to_owned());
        let resp = self
            .core
            .exchange_refresh_token(&rt)
            .request_async(&self.http)
            .await
            .map_err(|e| OidcError::Refresh(e.to_string()))?;
        Ok(TokenBundle {
            access_token: resp.access_token().secret().clone(),
            refresh_token: resp
                .refresh_token()
                .map(|t| t.secret().clone())
                .or_else(|| Some(refresh_token.to_owned())),
        })
    }

    #[tracing::instrument(skip_all)]
    pub async fn principal_from_access_token(&self, access_token: &str) -> Result<Principal, OidcError> {
        let claims: UserInfoClaims<OidcClaims, CoreGenderClaim> = self
            .core
            .user_info(AccessToken::new(access_token.to_owned()), None)
            .request_async(&self.http)
            .await
            .map_err(|e| OidcError::Userinfo(e.to_string()))?;

        Principal::from_userinfo(
            claims.subject().as_str(),
            claims.preferred_username().map(|u| u.as_str().to_owned()),
            claims.email().map(|e| e.as_str().to_owned()),
            &claims.additional_claims().effective_groups,
        )
        .map_err(OidcError::Userinfo)
    }
}
