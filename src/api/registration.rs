//! Registration policy (ADR 0037).
//!
//! nuck-server enforces the operator's policy at `/_nuck/v1/register`; the substrate's
//! own registration is closed off by the C-S proxy, so this is the only path. Users are
//! created through the Tuwunel admin API (ADR 0028).
//!
//! `approval` mode creates the account **deactivated** immediately (so no plaintext
//! password is ever stored) and only activates it on approval.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nuck_protocol::api::{
    RegisterDecisionResponse, RegisterPendingResponse, RegisterPolicy, RegisterRequest,
    RegisterRequestInfo, RegisterRequestsResponse, RegisterResponse, RegistrationMode,
};

use crate::projection::RegistrationRequestRow;
use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;
use super::privileged;

/// `GET /_nuck/v1/register/policy` — public, so a client can render the form.
pub async fn policy(State(state): State<Arc<AppState>>) -> Json<RegisterPolicy> {
    let mode = parse_mode(&state.config.registration_mode);
    Json(RegisterPolicy {
        mode,
        token_required: mode == RegistrationMode::Token,
        // v1 has no email verification or CAPTCHA (ADR 0037 verification).
        email_required: false,
        captcha: false,
    })
}

/// `POST /_nuck/v1/register` — apply the registration policy (ADR 0037).
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Response, ApiErrorResponse> {
    let mode = parse_mode(&state.config.registration_mode);
    let server = state.config.server_name.clone();
    let username = req.username.trim();
    let user_id = format!("@{username}:{server}");
    if ruma::UserId::parse(&user_id).is_err() {
        return Err(ApiErrorResponse::bad_request("invalid username"));
    }

    match mode {
        RegistrationMode::Closed => {
            return Err(ApiErrorResponse::forbidden("registration is closed"));
        }
        RegistrationMode::Token => {
            let expected = state.config.registration_token.as_deref();
            if expected.is_none() || req.token.as_deref() != expected {
                return Err(ApiErrorResponse::forbidden(
                    "a valid registration token is required",
                ));
            }
        }
        RegistrationMode::Open | RegistrationMode::Approval => {}
    }

    let approval = mode == RegistrationMode::Approval;
    state
        .substrate
        .admin_create_user(&user_id, &req.password, approval)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

    if approval {
        let id = uuid::Uuid::new_v4().to_string();
        privileged::projection(&state)?
            .insert_registration_request(&RegistrationRequestRow {
                id: id.clone(),
                username: username.to_owned(),
                user_id,
                created_at: privileged::now_ms(),
            })
            .await
            .map_err(privileged::internal)?;
        return Ok((
            StatusCode::ACCEPTED,
            Json(RegisterPendingResponse { request_id: id }),
        )
            .into_response());
    }

    // Active account: mint a session via admin login-as (ADR 0028).
    let (access_token, device_id) = state
        .substrate
        .admin_login(&user_id)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    Ok(Json(RegisterResponse {
        user_id,
        access_token,
        device_id,
        home_server: server,
    })
    .into_response())
}

/// `GET /_nuck/v1/register/requests` — the pending queue (admin, ADR 0037).
pub async fn list_requests(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
) -> Result<Json<RegisterRequestsResponse>, ApiErrorResponse> {
    require_admin(&state, &actor)?;
    let items = privileged::projection(&state)?
        .list_registration_requests()
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|row| RegisterRequestInfo {
            id: row.id,
            username: row.username,
            created_at: row.created_at.max(0) as u64,
        })
        .collect();
    Ok(Json(RegisterRequestsResponse { items, next_cursor: None }))
}

/// `POST /_nuck/v1/register/requests/{id}/approve` — activate the account (ADR 0037).
pub async fn approve(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<RegisterDecisionResponse>, ApiErrorResponse> {
    require_admin(&state, &actor)?;
    let request = privileged::projection(&state)?
        .get_registration_request(&id)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("no such registration request"))?;
    state
        .substrate
        .admin_set_deactivated(&request.user_id, false)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    privileged::projection(&state)?
        .delete_registration_request(&id)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(RegisterDecisionResponse { user_id: Some(request.user_id) }))
}

/// `POST /_nuck/v1/register/requests/{id}/deny` — drop the request (ADR 0037).
pub async fn deny(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<RegisterDecisionResponse>, ApiErrorResponse> {
    require_admin(&state, &actor)?;
    let _request = privileged::projection(&state)?
        .get_registration_request(&id)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("no such registration request"))?;
    // The account stays deactivated; drop the queue entry.
    privileged::projection(&state)?
        .delete_registration_request(&id)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(RegisterDecisionResponse { user_id: None }))
}

/// Require an operator admin (ADR 0037). Server admins are configured, not a role.
fn require_admin(state: &AppState, user_id: &str) -> Result<(), ApiErrorResponse> {
    if state.config.admin_users.iter().any(|admin| admin == user_id) {
        Ok(())
    } else {
        Err(ApiErrorResponse::forbidden("a server admin is required"))
    }
}

fn parse_mode(raw: &str) -> RegistrationMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" => RegistrationMode::Open,
        "approval" => RegistrationMode::Approval,
        "closed" => RegistrationMode::Closed,
        _ => RegistrationMode::Token,
    }
}
