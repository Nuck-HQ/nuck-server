//! `GET /_nuck/v1/me` — the caller's profile and Nuck settings (ADR 0013).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use nuck_protocol::api::Me;

use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;

/// The account-data event type holding a user's Nuck settings.
const SETTINGS_TYPE: &str = "space.nuck.settings.v1";

/// `GET /_nuck/v1/me` — resolve the caller's profile and settings **as the user**.
pub async fn me(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
) -> Result<Json<Me>, ApiErrorResponse> {
    let (display_name, avatar_url) = state
        .substrate
        .profile(&token, &user_id)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        .unwrap_or((None, None));

    // Unset settings are not an error — an account may never have written them.
    let settings = state
        .substrate
        .account_data(&token, &user_id, SETTINGS_TYPE)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        .unwrap_or(serde_json::Value::Null);

    Ok(Json(Me { user_id, display_name, avatar_url, settings }))
}
