//! Invites and joining (ADR 0011/0025).
//!
//! Invites are **host-authoritative**: the invite lives as `space.nuck.invite.v1` state
//! in the guild room, keyed by the **hash** of the code (the plaintext never federates).
//! A redeem force-joins the caller into the guild Space and its channels.
//!
//! `approval`-mode guilds (ADR 0025) take a `space.nuck.join_request.v1` instead; an
//! approver force-joins the applicant.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use nuck_protocol::api::{
    ActionResponse, CreateInviteRequest, CreateInviteResponse, InviteInfo, InvitePreview,
    InvitesResponse, JoinRequestCreate, JoinRequestInfo, JoinRequestResponse, JoinRequestsResponse,
    RedeemResponse,
};
use nuck_protocol::events::event_types as et;
use nuck_protocol::permissions::Permissions;
use sha2::{Digest, Sha256};

use crate::projection::InviteRow;

use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;
use super::privileged;

/// `POST /_nuck/v1/guilds/{guildId}/invites` — create an invite code (ADR 0011).
pub async fn create_invite(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> Result<Json<CreateInviteResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::CREATE_INSTANT_INVITE, "CREATE_INSTANT_INVITE")?;

    let code = new_invite_code();
    let code_hash = hash_code(&code);
    let expires_at = req
        .expires_in_ms
        .map(|ms| privileged::now_ms() + ms as i64);
    let content = serde_json::json!({
        "max_uses": req.max_uses,
        "uses": 0,
        "expires_at": expires_at,
        "created_by": actor,
    });

    let bot = privileged::action_bot(&state)?;
    bot.write_state(&guild_id, et::INVITE, &code_hash, content)
        .await
        .map_err(privileged::internal)?;

    // The plaintext code is local-only (the federated event carries only the hash), so
    // record it in the projection directly. The AppService echo cannot know it, and the
    // upsert preserves this column (`COALESCE`), so a later echo will not clear it.
    privileged::projection(&state)?
        .upsert_invite(&InviteRow {
            guild_id: guild_id.clone(),
            code_hash: code_hash.clone(),
            code: Some(code.clone()),
            max_uses: req.max_uses.map(|u| u as i32),
            uses: 0,
            expires_at,
            created_by: Some(actor.clone()),
            event_id: format!("local:{code_hash}"),
        })
        .await
        .map_err(privileged::internal)?;

    Ok(Json(CreateInviteResponse {
        url: invite_url(&state, &code),
        code,
        expires_at: expires_at.map(|at| at.max(0) as u64),
    }))
}

/// `GET /_nuck/v1/guilds/{guildId}/invites` — list a guild's invites (ADR 0011).
pub async fn list_invites(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
) -> Result<Json<InvitesResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_GUILD, "MANAGE_GUILD")?;

    let items = privileged::projection(&state)?
        .list_invites(&guild_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|row| InviteInfo {
            // Host-created invites carry the plaintext; a federated-only row falls back
            // to the hash (the real code was never known locally).
            code: row.code.unwrap_or(row.code_hash),
            uses: row.uses.max(0) as u32,
            max_uses: row.max_uses.map(|u| u.max(0) as u32),
            expires_at: row.expires_at.map(|at| at.max(0) as u64),
            created_by: row.created_by.unwrap_or_default(),
        })
        .collect();
    Ok(Json(InvitesResponse { items, next_cursor: None }))
}

/// `DELETE /_nuck/v1/guilds/{guildId}/invites/{code}` — revoke an invite.
pub async fn revoke_invite(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((guild_id, code)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_GUILD, "MANAGE_GUILD")?;

    let code_hash = hash_code(&code);
    let bot = privileged::action_bot(&state)?;
    let event_id = bot
        .write_state(&guild_id, et::INVITE, &code_hash, serde_json::json!({}))
        .await
        .map_err(privileged::internal)?;
    privileged::projection(&state)?
        .delete_invite(&guild_id, &code_hash)
        .await
        .map_err(privileged::internal)?;
    privileged::record(&state, &guild_id, &actor, "invite.revoke", Some(&code_hash), &event_id)
        .await?;
    Ok(Json(ActionResponse { event_id }))
}

/// `GET /_nuck/v1/invites/{code}` — public pre-redeem preview (ADR 0011).
pub async fn preview_invite(
    State(state): State<Arc<AppState>>,
    Path(code): Path<String>,
) -> Result<Json<InvitePreview>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    let invite = projection
        .get_invite_by_hash(&hash_code(&code))
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("unknown invite"))?;
    if is_expired(&invite) {
        return Err(ApiErrorResponse::not_found("this invite has expired"));
    }

    let guild = projection
        .get_guild(&invite.guild_id)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("the guild no longer exists"))?;
    let member_count = projection
        .count_guild_members(&invite.guild_id)
        .await
        .map_err(privileged::internal)?;

    Ok(Json(InvitePreview {
        guild_id: invite.guild_id,
        guild_name: guild.name,
        guild_icon_url: None,
        member_count: member_count.max(0) as u64,
        join_mode: parse_join_mode(guild.join_mode.as_deref()),
    }))
}

/// `POST /_nuck/v1/invites/{code}/redeem` — join via an invite (ADR 0011).
pub async fn redeem_invite(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(code): Path<String>,
) -> Result<Json<RedeemResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    let invite = projection
        .get_invite_by_hash(&hash_code(&code))
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("unknown invite"))?;
    if is_expired(&invite) {
        return Err(ApiErrorResponse::forbidden("this invite has expired"));
    }
    if let Some(max) = invite.max_uses {
        if invite.uses >= max {
            return Err(ApiErrorResponse::forbidden("this invite has no uses left"));
        }
    }
    if projection
        .get_ban(&invite.guild_id, &user_id)
        .await
        .map_err(privileged::internal)?
        .is_some()
    {
        return Err(ApiErrorResponse::forbidden("you are banned from this guild"));
    }

    let channels = projection
        .list_channels(&invite.guild_id)
        .await
        .map_err(privileged::internal)?;
    force_join(&state, &invite.guild_id, &channels.iter().map(|c| c.channel_id.clone()).collect::<Vec<_>>(), &user_id).await?;

    // Record the redemption (host-authoritative use counting).
    let bot = privileged::action_bot(&state)?;
    let content = serde_json::json!({
        "max_uses": invite.max_uses,
        "uses": invite.uses + 1,
        "expires_at": invite.expires_at,
        "created_by": invite.created_by.clone().unwrap_or_default(),
    });
    bot.write_state(&invite.guild_id, et::INVITE, &invite.code_hash, content)
        .await
        .map_err(privileged::internal)?;

    Ok(Json(RedeemResponse {
        guild_id: invite.guild_id,
        channel_id: channels.first().map(|c| c.channel_id.clone()),
    }))
}

/// `POST /_nuck/v1/guilds/{guildId}/join-requests` — request to join (ADR 0025).
pub async fn create_join_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
    Json(req): Json<JoinRequestCreate>,
) -> Result<Json<JoinRequestResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &user_id).await?;
    if access.is_member() {
        return Err(ApiErrorResponse::new(
            axum::http::StatusCode::CONFLICT,
            "M_CONFLICT",
            "you are already a member",
        ));
    }
    if access.guild.join_mode.as_deref() != Some("approval") {
        return Err(ApiErrorResponse::bad_request(
            "this guild does not take join requests",
        ));
    }

    let bot = privileged::action_bot(&state)?;
    let content = serde_json::json!({
        "message": req.message,
        "status": "pending",
        "requested_at": privileged::now_ms(),
    });
    bot.write_state(&guild_id, et::JOIN_REQUEST, &crate::keys::encode_user_key(&user_id), content)
        .await
        .map_err(privileged::internal)?;

    Ok(Json(JoinRequestResponse {
        user_id,
        status: nuck_protocol::events::JoinRequestStatus::Pending,
    }))
}

/// `GET /_nuck/v1/guilds/{guildId}/join-requests` — the pending queue (ADR 0025).
pub async fn list_join_requests(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
) -> Result<Json<JoinRequestsResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_GUILD, "MANAGE_GUILD")?;

    let items = privileged::projection(&state)?
        .list_join_requests(&guild_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|row| JoinRequestInfo {
            user_id: row.user_id,
            message: row.message,
            status: parse_join_status(&row.status),
            requested_at: row.requested_at.max(0) as u64,
        })
        .collect();
    Ok(Json(JoinRequestsResponse { items, next_cursor: None }))
}

/// `POST /_nuck/v1/guilds/{guildId}/join-requests/{userId}/approve` (ADR 0025).
pub async fn approve_join_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((guild_id, applicant)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let event_id = decide_join_request(&state, &guild_id, &actor, &applicant, "approved").await?;
    // Approval joins the applicant.
    let channels = privileged::projection(&state)?
        .list_channels(&guild_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|c| c.channel_id)
        .collect::<Vec<_>>();
    force_join(&state, &guild_id, &channels, &applicant).await?;
    Ok(Json(ActionResponse { event_id }))
}

/// `POST /_nuck/v1/guilds/{guildId}/join-requests/{userId}/deny` (ADR 0025).
pub async fn deny_join_request(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((guild_id, applicant)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let event_id = decide_join_request(&state, &guild_id, &actor, &applicant, "denied").await?;
    Ok(Json(ActionResponse { event_id }))
}

// --- Helpers ---------------------------------------------------------------

/// Write the decided status onto an existing join request (ADR 0025).
async fn decide_join_request(
    state: &AppState,
    guild_id: &str,
    actor: &str,
    applicant: &str,
    status: &str,
) -> Result<String, ApiErrorResponse> {
    let access = privileged::load(state, guild_id, actor).await?;
    privileged::require_local(state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_GUILD, "MANAGE_GUILD")?;

    let existing = privileged::projection(state)?
        .get_join_request(guild_id, applicant)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("no such join request"))?;

    let bot = privileged::action_bot(state)?;
    let content = serde_json::json!({
        "message": existing.message,
        "status": status,
        "requested_at": existing.requested_at,
    });
    let event_id = bot
        .write_state(guild_id, et::JOIN_REQUEST, &crate::keys::encode_user_key(applicant), content)
        .await
        .map_err(privileged::internal)?;
    privileged::record(
        state,
        guild_id,
        actor,
        &format!("join_request.{status}"),
        Some(applicant),
        &event_id,
    )
    .await?;
    Ok(event_id)
}

/// Add `user_id` to the guild and its channels (ADR 0011).
///
/// Redemption and approval are host-authoritative. The bot **invites** the user (it
/// holds the `invite` power, ADR 0041), which needs no admin token; the client then
/// accepts. A user already in a room is a no-op.
async fn force_join(
    state: &AppState,
    guild_id: &str,
    channels: &[String],
    user_id: &str,
) -> Result<(), ApiErrorResponse> {
    let bot = privileged::action_bot(state)?;
    for room_id in std::iter::once(guild_id).chain(channels.iter().map(String::as_str)) {
        if let Err(e) = bot.invite(room_id, user_id).await {
            tracing::warn!(room = %room_id, user = %user_id, "guild invite failed: {e}");
        }
    }
    Ok(())
}

/// A fresh opaque invite code: `nuck_` + base62 of 128 random bits (~22 chars).
fn new_invite_code() -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut n = u128::from_le_bytes(*uuid::Uuid::new_v4().as_bytes());
    let mut out = Vec::new();
    while n > 0 {
        out.push(ALPHABET[(n % 62) as usize]);
        n /= 62;
    }
    out.reverse();
    format!("nuck_{}", String::from_utf8(out).unwrap_or_default())
}

/// The hex SHA-256 of an invite code — the federated state key (ADR 0011).
fn hash_code(code: &str) -> String {
    let digest = Sha256::digest(code.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn invite_url(state: &AppState, code: &str) -> String {
    format!("https://{}/invite/{}", state.config.server_name, code)
}

fn is_expired(invite: &InviteRow) -> bool {
    invite
        .expires_at
        .is_some_and(|at| at <= privileged::now_ms())
}

fn parse_join_mode(mode: Option<&str>) -> nuck_protocol::events::JoinMode {
    use nuck_protocol::events::JoinMode;
    match mode {
        Some("open") => JoinMode::Open,
        Some("approval") => JoinMode::Approval,
        _ => JoinMode::InviteOnly,
    }
}

fn parse_join_status(status: &str) -> nuck_protocol::events::JoinRequestStatus {
    use nuck_protocol::events::JoinRequestStatus;
    match status {
        "approved" => JoinRequestStatus::Approved,
        "denied" => JoinRequestStatus::Denied,
        _ => JoinRequestStatus::Pending,
    }
}
