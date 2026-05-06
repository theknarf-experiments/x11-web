//! OIDC routes layered onto the main axum router.
//!
//! `/auth/login`     — 302 to the IdP's authorize endpoint with
//!                      PKCE challenge + nonce stashed in the
//!                      session for the round-trip.
//! `/auth/callback`  — the IdP bounces the user back here with
//!                      `?code=...&state=...`. We exchange the
//!                      code, verify the ID token, persist
//!                      `{sub, email}` into the session, and 302
//!                      back to `/`.
//! `/auth/logout`    — clear the session, 200.
//! `/auth/me`        — JSON `{sub, email} | null`. The frontend
//!                      polls this on boot to discover the
//!                      current user (if any).

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tower_sessions::Session;
use x11_web_auth::{AuthenticatedUser, Authenticator, PendingLogin};

const SESSION_USER_KEY: &str = "auth.user";
const SESSION_PENDING_KEY: &str = "auth.pending";

/// Per-server auth state. `Some` when `OIDC_ISSUER` was set at
/// startup; `None` triggers the anonymous-only paths below
/// (login/callback respond with 503; `/auth/me` always returns
/// `null`).
#[derive(Clone)]
pub struct AuthState {
    pub authenticator: Option<Arc<Authenticator>>,
}

impl AuthState {
    pub fn new(authenticator: Option<Arc<Authenticator>>) -> Self {
        Self { authenticator }
    }
}

pub fn router() -> Router<AuthState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

async fn login(State(state): State<AuthState>, session: Session) -> Response {
    let Some(auth) = state.authenticator.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "OIDC not configured on this backend",
        )
            .into_response();
    };
    let (url, pending) = match auth.begin_login().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "/auth/login: begin_login failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("login init failed: {e}"),
            )
                .into_response();
        }
    };
    if let Err(e) = session.insert(SESSION_PENDING_KEY, &pending).await {
        tracing::warn!(error = %e, "/auth/login: failed to write pending login to session");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("session write failed: {e}"),
        )
            .into_response();
    }
    Redirect::to(url.as_str()).into_response()
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(
    State(state): State<AuthState>,
    Query(q): Query<CallbackQuery>,
    session: Session,
) -> Response {
    let Some(auth) = state.authenticator.clone() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "OIDC not configured").into_response();
    };
    if let Some(err) = q.error.as_deref() {
        tracing::info!(?err, ?q.error_description, "OIDC callback returned error");
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "auth provider returned error: {err}{}",
                q.error_description
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            ),
        )
            .into_response();
    }
    let (Some(code), Some(state_param)) = (q.code, q.state) else {
        return (
            StatusCode::BAD_REQUEST,
            "missing `code` or `state` query param",
        )
            .into_response();
    };
    let pending: PendingLogin = match session.remove(SESSION_PENDING_KEY).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                "no pending login in session — start at /auth/login",
            )
                .into_response();
        }
        Err(e) => {
            tracing::warn!(error = %e, "/auth/callback: session read failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "session read failed").into_response();
        }
    };
    let user = match auth.complete_login(code, state_param, pending).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = %e, "/auth/callback: complete_login failed");
            return (
                StatusCode::UNAUTHORIZED,
                format!("token exchange / verification failed: {e}"),
            )
                .into_response();
        }
    };
    if let Err(e) = session.insert(SESSION_USER_KEY, &user).await {
        tracing::warn!(error = %e, "/auth/callback: session write failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "session write failed").into_response();
    }
    tracing::info!(sub = %user.sub, ?user.email, "user signed in");
    // Bounce back to the SPA root. The exact URL is env-driven
    // (`OIDC_POST_LOGIN_REDIRECT`) because in dev / e2e the SPA
    // sits on a different port from the backend.
    Redirect::to(auth.post_login_redirect()).into_response()
}

async fn logout(session: Session) -> Response {
    if let Err(e) = session.flush().await {
        tracing::warn!(error = %e, "/auth/logout: session flush failed");
        return (StatusCode::INTERNAL_SERVER_ERROR, "logout failed").into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn me(session: Session) -> Json<Option<AuthenticatedUser>> {
    match session.get::<AuthenticatedUser>(SESSION_USER_KEY).await {
        Ok(user) => Json(user),
        Err(_) => Json(None),
    }
}
