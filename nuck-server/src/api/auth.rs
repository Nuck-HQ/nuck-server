//! Caller authentication (ADR 0009/0028).
//!
//! The Nuck client authenticates with a **Matrix access token**; nuck-server validates
//! it against the substrate and acts **as that user**. Matrix user ids are canonical
//! Nuck identity, so there is no second login (ADR 0009). The token is kept so handlers
//! can make substrate calls on the user's behalf (guild creation, proxying).

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header;
use axum::http::request::Parts;

use crate::state::AppState;

use super::error::ApiErrorResponse;

/// The authenticated caller.
pub struct AuthenticatedUser {
    /// The caller's Matrix user id.
    pub user_id: String,
    /// The caller's access token, for acting as them against the substrate.
    pub token: String,
}

impl FromRequestParts<Arc<AppState>> for AuthenticatedUser {
    type Rejection = ApiErrorResponse;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)
            .ok_or_else(ApiErrorResponse::unauthorized)?
            .to_owned();

        match state.substrate.whoami(&token).await {
            Ok(Some(user_id)) => Ok(AuthenticatedUser { user_id, token }),
            // The substrate rejected the token: an invalid or expired session.
            Ok(None) => Err(ApiErrorResponse::unauthorized()),
            // The substrate is unreachable or misbehaved — not the caller's fault.
            Err(e) => {
                tracing::warn!("token validation failed: {e}");
                Err(ApiErrorResponse::unavailable("the homeserver is unavailable"))
            }
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
