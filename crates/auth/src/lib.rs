//! OIDC authentication, configured by env so any standard
//! OAuth2/OIDC provider works without code changes.
//!
//! The host backend reads `OidcConfig::from_env()` once at startup;
//! when it returns `None` (because `OIDC_ISSUER` is unset) the server
//! runs in anonymous-only mode. When configured, the
//! [`Authenticator`] handles discovery, JWKS caching, the
//! Authorization Code + PKCE flow, and ID-token verification, and
//! exposes [`AuthenticatedUser`] to callers.

use std::sync::Arc;

use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::reqwest::Client as OidcHttpClient;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EmptyAdditionalClaims, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::OnceCell;
use tracing::info;

/// Subset of an authenticated identity the rest of the backend
/// cares about. Sourced from the OIDC `sub` (stable per-user) and
/// optional `email` claims after JWT verification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthenticatedUser {
    pub sub: String,
    pub email: Option<String>,
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("OIDC discovery failed: {0}")]
    Discovery(String),
    #[error("invalid env: {0}")]
    Config(String),
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("ID token verification failed: {0}")]
    Verify(String),
    #[error("HTTP client error: {0}")]
    Http(String),
}

/// OIDC env-derived configuration. `None` from `from_env()` means
/// anonymous-only mode; the backend should still serve all
/// existing routes and just not surface `/auth/login`-style
/// redirects.
#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    /// Where the user is bounced after `/auth/callback` finishes.
    /// Same-origin `/` works when the backend serves the SPA; in
    /// the dev / e2e setup where the SPA is on a different port,
    /// this needs to be the SPA's absolute URL so the browser
    /// lands back on the React app.
    pub post_login_redirect: String,
}

impl OidcConfig {
    /// Read OIDC config from env. Returns `None` when `OIDC_ISSUER`
    /// is unset or empty — i.e. the operator hasn't configured an
    /// identity provider, so the backend stays in anonymous mode.
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var("OIDC_ISSUER").ok()?;
        if issuer.trim().is_empty() {
            return None;
        }
        let client_id = std::env::var("OIDC_CLIENT_ID").unwrap_or_else(|_| "x11-web".into());
        let client_secret = std::env::var("OIDC_CLIENT_SECRET")
            .ok()
            .filter(|s| !s.is_empty());
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:3001/auth/callback".into());
        let post_login_redirect =
            std::env::var("OIDC_POST_LOGIN_REDIRECT").unwrap_or_else(|_| "/".into());
        Some(Self {
            issuer,
            client_id,
            client_secret,
            redirect_uri,
            post_login_redirect,
        })
    }
}

/// In-progress login. Stored in the user's session between
/// `/auth/login` (redirect to IdP) and `/auth/callback` (code
/// exchange) so we can verify the CSRF token and PKCE verifier
/// match the request that started the flow.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingLogin {
    pub csrf_token: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

/// Authenticator wraps the discovered OIDC client. Discovery (the
/// blocking call to `/.well-known/openid-configuration` plus the
/// JWKS fetch) is lazy — the first call to `client()` performs
/// it; subsequent calls reuse the cached metadata.
pub struct Authenticator {
    config: OidcConfig,
    http: OidcHttpClient,
    client: OnceCell<
        CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
    >,
}

impl Authenticator {
    pub fn new(config: OidcConfig) -> Result<Arc<Self>, AuthError> {
        // Disable redirects on the OIDC HTTP client — the
        // openidconnect crate notes this prevents SSRF if a
        // misconfigured IdP returns a 3xx pointing somewhere
        // sensitive.
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AuthError::Http(e.to_string()))?;
        Ok(Arc::new(Self {
            config,
            http,
            client: OnceCell::new(),
        }))
    }

    /// Lazily-initialised OIDC client. The first invocation does
    /// discovery; we keep the result in a `OnceCell` so subsequent
    /// /auth/login + /auth/callback calls don't re-fetch.
    async fn client(
        &self,
    ) -> Result<
        &CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
        AuthError,
    > {
        self.client
            .get_or_try_init(|| async {
                let issuer = IssuerUrl::new(self.config.issuer.clone())
                    .map_err(|e| AuthError::Config(format!("OIDC_ISSUER: {e}")))?;
                let metadata = CoreProviderMetadata::discover_async(issuer, &self.http)
                    .await
                    .map_err(|e| AuthError::Discovery(e.to_string()))?;
                info!(
                    "OIDC discovery complete; issuer={} authorization_endpoint={}",
                    self.config.issuer,
                    metadata.authorization_endpoint().as_str()
                );
                let secret = self.config.client_secret.clone().map(ClientSecret::new);
                let redirect_url = RedirectUrl::new(self.config.redirect_uri.clone())
                    .map_err(|e| AuthError::Config(format!("OIDC_REDIRECT_URI: {e}")))?;
                let client = CoreClient::from_provider_metadata(
                    metadata,
                    ClientId::new(self.config.client_id.clone()),
                    secret,
                )
                .set_redirect_uri(redirect_url);
                Ok::<_, AuthError>(client)
            })
            .await
    }

    /// Build the IdP authorize URL and the matching pending-login
    /// state the caller stores in the session. Caller redirects
    /// the user to the URL; the IdP bounces back to
    /// `/auth/callback?code=…` on success.
    pub async fn begin_login(&self) -> Result<(url::Url, PendingLogin), AuthError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, csrf, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".into()))
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok((
            auth_url,
            PendingLogin {
                csrf_token: csrf.secret().clone(),
                pkce_verifier: verifier.secret().clone(),
                nonce: nonce.secret().clone(),
            },
        ))
    }

    /// Complete the flow: exchange the code, verify the ID token,
    /// extract the `sub` + `email` claims.
    /// URL to redirect the user to after a successful callback —
    /// the SPA root, in dev/e2e on a different port from the
    /// backend.
    pub fn post_login_redirect(&self) -> &str {
        &self.config.post_login_redirect
    }

    pub async fn complete_login(
        &self,
        code: String,
        state: String,
        pending: PendingLogin,
    ) -> Result<AuthenticatedUser, AuthError> {
        if state != pending.csrf_token {
            return Err(AuthError::Verify("CSRF state mismatch".into()));
        }
        let client = self.client().await?;
        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|e| AuthError::Exchange(e.to_string()))?
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier))
            .request_async(&self.http)
            .await
            .map_err(|e| AuthError::Exchange(e.to_string()))?;
        let id_token = token_response
            .id_token()
            .ok_or_else(|| AuthError::Verify("no ID token in response".into()))?;
        let nonce = Nonce::new(pending.nonce);
        let claims = id_token
            .claims(&client.id_token_verifier(), &nonce)
            .map_err(|e| AuthError::Verify(e.to_string()))?;
        let _ = token_response.access_token(); // available if downstream needs it
        let _: &EmptyAdditionalClaims = claims.additional_claims();
        Ok(AuthenticatedUser {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|e| e.as_str().to_string()),
        })
    }
}
