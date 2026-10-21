//! Per-channel permission overrides (ADR 0020/0041).
//!
//! Same pipeline as guild actions: authenticate → authorize → the action bot writes
//! the `space.nuck.override.v1` state event **in the channel room**. Overrides are
//! part of the guild's permission model, so they are guild state that federates, not
//! local configuration.
//!
//! **Removal** neutralises the override (allow = deny = 0): Matrix has no "delete
//! state" operation, and an all-zero override resolves to exactly the same
//! permissions as no override at all, so the neutral event is the honest equivalent.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use nuck_protocol::api::{ActionResponse, ChannelOverride, ChannelOverridesResponse, SetChannelOverrideRequest};
use nuck_protocol::events::event_types as et;
use nuck_protocol::permissions::Permissions;

use crate::authorization::{self, GuildAccess};
use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;
use super::privileged;

/// `PUT /_nuck/v1/channels/{channelId}/overrides/{target}` — set an override.
pub async fn set_override(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((channel_id, target)): Path<(String, String)>,
    Json(req): Json<SetChannelOverrideRequest>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let access = channel_access(&state, &channel_id, &actor).await?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_ROLES, "MANAGE_ROLES")?;
    if !access.can_grant(req.allow) {
        return Err(ApiErrorResponse::forbidden(
            "you cannot grant a permission you do not have",
        ));
    }
    authorize_target(&state, &access, &target).await?;

    let bot = privileged::action_bot(&state)?;
    let content = serde_json::json!({ "allow": req.allow, "deny": req.deny });
    let event_id = bot
        .write_state(&channel_id, et::OVERRIDE, &target, content)
        .await
        .map_err(privileged::internal)?;

    privileged::record(
        &state,
        &access.guild.guild_id,
        &actor,
        "override.set",
        Some(&scoped(&channel_id, &target)),
        &event_id,
    )
    .await?;
    Ok(Json(ActionResponse { event_id }))
}

/// `DELETE /_nuck/v1/channels/{channelId}/overrides/{target}` — remove an override.
pub async fn remove_override(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((channel_id, target)): Path<(String, String)>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let access = channel_access(&state, &channel_id, &actor).await?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_ROLES, "MANAGE_ROLES")?;
    authorize_target(&state, &access, &target).await?;

    let bot = privileged::action_bot(&state)?;
    let content = serde_json::json!({ "allow": Permissions::NONE, "deny": Permissions::NONE });
    let event_id = bot
        .write_state(&channel_id, et::OVERRIDE, &target, content)
        .await
        .map_err(privileged::internal)?;

    privileged::record(
        &state,
        &access.guild.guild_id,
        &actor,
        "override.remove",
        Some(&scoped(&channel_id, &target)),
        &event_id,
    )
    .await?;
    Ok(Json(ActionResponse { event_id }))
}

/// `GET /_nuck/v1/channels/{channelId}/overrides` — the channel's overrides.
///
/// Gated by **channel membership** (ADR 0041: channel access is room membership, not
/// a mirrored permission), so a non-member cannot read a guild's permission layout.
pub async fn list_overrides(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(channel_id): Path<String>,
) -> Result<Json<ChannelOverridesResponse>, ApiErrorResponse> {
    privileged::projection(&state)?
        .get_channel(&channel_id)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("unknown channel"))?;

    if privileged::membership(&state, &channel_id, &actor).await?.as_deref() != Some("join") {
        return Err(ApiErrorResponse::forbidden(
            "you are not a member of this channel",
        ));
    }

    let rows = privileged::projection(&state)?
        .list_overrides(&channel_id)
        .await
        .map_err(privileged::internal)?;
    let overrides = rows
        .into_iter()
        .map(|row| ChannelOverride {
            target: row.target,
            allow: Permissions::from_bits(row.allow as u64),
            deny: Permissions::from_bits(row.deny as u64),
        })
        .collect();

    Ok(Json(ChannelOverridesResponse { overrides }))
}

/// Resolve the channel's guild and the caller's standing in it.
async fn channel_access(
    state: &AppState,
    channel_id: &str,
    actor: &str,
) -> Result<GuildAccess, ApiErrorResponse> {
    let channel = privileged::projection(state)?
        .get_channel(channel_id)
        .await
        .map_err(privileged::internal)?
        .ok_or_else(|| ApiErrorResponse::not_found("unknown channel"))?;

    let access = privileged::load(state, &channel.guild_id, actor).await?;
    privileged::require_local(state, &access)?;
    Ok(access)
}

/// Enforce ADR 0020's hierarchy on the override target: a role must sit strictly below
/// the caller's highest, and a member must be outranked.
async fn authorize_target(
    state: &AppState,
    access: &GuildAccess,
    target: &str,
) -> Result<(), ApiErrorResponse> {
    if authorization::is_user_target(target) {
        // `is_user_target` already parsed it; re-parse to keep the type.
        ruma::UserId::parse(target)
            .map_err(|_| ApiErrorResponse::bad_request("invalid user id"))?;
        let roles = privileged::member_roles(state, &access.guild.guild_id, target).await?;
        if !access.outranks_member(&roles) {
            return Err(ApiErrorResponse::forbidden(
                "cannot override a member who is not below you",
            ));
        }
    } else {
        let position = access
            .role_override_position(target)
            .ok_or_else(|| ApiErrorResponse::bad_request(format!("unknown role: {target}")))?;
        if !access.outranks_position(position) {
            return Err(ApiErrorResponse::forbidden(
                "cannot override a role at or above your highest role",
            ));
        }
    }
    Ok(())
}

/// The audit target for a channel-scoped override.
fn scoped(channel_id: &str, target: &str) -> String {
    format!("{channel_id}#{target}")
}
