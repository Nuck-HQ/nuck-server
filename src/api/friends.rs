//! Friends, blocking, and DMs (ADR 0030).
//!
//! v1 is single-server, so friendship/block state lives in the projection (see
//! `docs/protocol/friends-dms.md` and migration 0007). Friend requests are additionally
//! delivered as to-device `space.nuck.friend_request.v1` / `…friend_response.v1` events,
//! so the mechanism federates when federation lands.
//!
//! Blocking suppresses new DMs and friend requests (ADR 0030): it is **not** a security
//! boundary and never affects guild membership.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use nuck_protocol::api::{
    ActionResponse, BlocksResponse, CreateDmRequest, CreateDmResponse, FriendInfo,
    FriendRequestCreate, FriendRequestDirection, FriendRequestInfo, FriendRequestsResponse,
    FriendsResponse,
};
use nuck_protocol::events::event_types as et;

use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;
use super::privileged;

/// `GET /_nuck/v1/friends` — the caller's friends (ADR 0030).
pub async fn list_friends(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
) -> Result<Json<FriendsResponse>, ApiErrorResponse> {
    let friends = privileged::projection(&state)?
        .list_friends(&user_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|(user_id, since)| FriendInfo {
            user_id,
            display_name: None,
            since: since.max(0) as u64,
        })
        .collect();
    Ok(Json(FriendsResponse { friends }))
}

/// `GET /_nuck/v1/friends/requests` — pending requests, both directions (ADR 0030).
pub async fn list_friend_requests(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
) -> Result<Json<FriendRequestsResponse>, ApiErrorResponse> {
    let requests = privileged::projection(&state)?
        .list_friend_requests(&user_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|(from, to, message, created_at)| {
            let incoming = to == user_id;
            FriendRequestInfo {
                user_id: if incoming { from } else { to },
                message,
                direction: if incoming {
                    FriendRequestDirection::Incoming
                } else {
                    FriendRequestDirection::Outgoing
                },
                created_at: created_at.max(0) as u64,
            }
        })
        .collect();
    Ok(Json(FriendRequestsResponse { requests }))
}

/// `POST /_nuck/v1/friends/requests` — send a friend request (ADR 0030).
pub async fn send_friend_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Json(req): Json<FriendRequestCreate>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let target = req.user_id;
    validate_target(&user_id, &target)?;

    let projection = privileged::projection(&state)?;
    if projection
        .is_blocked_either(&user_id, &target)
        .await
        .map_err(privileged::internal)?
    {
        return Err(ApiErrorResponse::forbidden("you cannot interact with this user"));
    }
    if projection
        .are_friends(&user_id, &target)
        .await
        .map_err(privileged::internal)?
    {
        return Err(conflict("you are already friends"));
    }
    let outgoing = projection
        .get_friend_request(&user_id, &target)
        .await
        .map_err(privileged::internal)?;
    let incoming = projection
        .get_friend_request(&target, &user_id)
        .await
        .map_err(privileged::internal)?;
    if outgoing.is_some() || incoming.is_some() {
        return Err(conflict("a friend request is already pending"));
    }

    // Deliver the request point-to-point (federates natively).
    let content = serde_json::json!({ "message": req.message });
    let messages = serde_json::json!({ target.clone(): { "*": content } });
    let txn = format!("nuck-friend-{}", uuid::Uuid::new_v4());
    state
        .substrate
        .send_to_device(&token, et::FRIEND_REQUEST, &txn, messages)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

    projection
        .insert_friend_request(&user_id, &target, req.message.as_deref(), privileged::now_ms())
        .await
        .map_err(privileged::internal)?;
    Ok(Json(ActionResponse { event_id: format!("friend_request:{target}") }))
}

/// `POST /_nuck/v1/friends/requests/{userId}/accept` — accept (ADR 0030).
pub async fn accept_friend_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Path(other): Path<String>,
) -> Result<Json<FriendsResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    if projection
        .get_friend_request(&other, &user_id)
        .await
        .map_err(privileged::internal)?
        .is_none()
    {
        return Err(ApiErrorResponse::not_found("no such friend request"));
    }
    let now = privileged::now_ms();
    projection
        .insert_friendship(&user_id, &other, now)
        .await
        .map_err(privileged::internal)?;
    projection
        .delete_friend_request(&other, &user_id)
        .await
        .map_err(privileged::internal)?;

    let messages = serde_json::json!({ other.clone(): { "*": { "accepted": true } } });
    let txn = format!("nuck-friend-{}", uuid::Uuid::new_v4());
    state
        .substrate
        .send_to_device(&token, et::FRIEND_RESPONSE, &txn, messages)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

    list_friends(State(state), AuthenticatedUser { user_id, token }).await
}

/// `POST /_nuck/v1/friends/requests/{userId}/decline` — decline (ADR 0030).
pub async fn decline_friend_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Path(other): Path<String>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    if projection
        .get_friend_request(&other, &user_id)
        .await
        .map_err(privileged::internal)?
        .is_none()
    {
        return Err(ApiErrorResponse::not_found("no such friend request"));
    }
    projection
        .delete_friend_request(&other, &user_id)
        .await
        .map_err(privileged::internal)?;

    let messages = serde_json::json!({ other.clone(): { "*": { "accepted": false } } });
    let txn = format!("nuck-friend-{}", uuid::Uuid::new_v4());
    state
        .substrate
        .send_to_device(&token, et::FRIEND_RESPONSE, &txn, messages)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    Ok(Json(ActionResponse { event_id: format!("friend_decline:{other}") }))
}

/// `DELETE /_nuck/v1/friends/{userId}` — remove a friend (ADR 0030).
pub async fn remove_friend(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(other): Path<String>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    if !projection
        .are_friends(&user_id, &other)
        .await
        .map_err(privileged::internal)?
    {
        return Err(ApiErrorResponse::not_found("not a friend"));
    }
    projection
        .delete_friendship(&user_id, &other)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(ActionResponse { event_id: format!("friend_remove:{other}") }))
}

/// `GET /_nuck/v1/blocks` — the caller's block list (ADR 0030).
pub async fn list_blocks(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
) -> Result<Json<BlocksResponse>, ApiErrorResponse> {
    let blocked = privileged::projection(&state)?
        .list_blocks(&user_id)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(BlocksResponse { blocked }))
}

/// `PUT /_nuck/v1/blocks/{userId}` — block a user (ADR 0030). Idempotent.
pub async fn block_user(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(other): Path<String>,
) -> Result<Json<BlocksResponse>, ApiErrorResponse> {
    validate_target(&user_id, &other)?;
    let projection = privileged::projection(&state)?;
    projection
        .insert_block(&user_id, &other)
        .await
        .map_err(privileged::internal)?;
    // Blocking is a superset of unfriend + request decline.
    projection
        .delete_friendship(&user_id, &other)
        .await
        .map_err(privileged::internal)?;
    projection
        .delete_friend_requests_between(&user_id, &other)
        .await
        .map_err(privileged::internal)?;
    let blocked = projection
        .list_blocks(&user_id)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(BlocksResponse { blocked }))
}

/// `DELETE /_nuck/v1/blocks/{userId}` — unblock (ADR 0030). Idempotent.
pub async fn unblock_user(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(other): Path<String>,
) -> Result<Json<BlocksResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    projection
        .delete_block(&user_id, &other)
        .await
        .map_err(privileged::internal)?;
    let blocked = projection
        .list_blocks(&user_id)
        .await
        .map_err(privileged::internal)?;
    Ok(Json(BlocksResponse { blocked }))
}

/// `POST /_nuck/v1/dm` — open a direct room with a user (ADR 0030).
pub async fn create_dm(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Json(req): Json<CreateDmRequest>,
) -> Result<Json<CreateDmResponse>, ApiErrorResponse> {
    let target = req.user_id;
    validate_target(&user_id, &target)?;
    if privileged::projection(&state)?
        .is_blocked_either(&user_id, &target)
        .await
        .map_err(privileged::internal)?
    {
        return Err(ApiErrorResponse::forbidden("you cannot message this user"));
    }

    // A plain Matrix direct room, created as the caller; E2EE is set up by the client.
    let room_id = state
        .substrate
        .create_room(
            &token,
            None,
            serde_json::json!({
                "is_direct": true,
                "preset": "trusted_private_chat",
                "invite": [target],
            }),
        )
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    Ok(Json(CreateDmResponse { room_id }))
}

// --- Helpers ---------------------------------------------------------------

fn validate_target(user_id: &str, target: &str) -> Result<(), ApiErrorResponse> {
    if target == user_id {
        return Err(ApiErrorResponse::bad_request("you cannot target yourself"));
    }
    ruma::UserId::parse(target)
        .map(|_| ())
        .map_err(|_| ApiErrorResponse::bad_request("invalid user id"))
}

fn conflict(message: &str) -> ApiErrorResponse {
    ApiErrorResponse::new(axum::http::StatusCode::CONFLICT, "M_CONFLICT", message)
}
