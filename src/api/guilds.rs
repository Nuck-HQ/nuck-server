//! Privileged guild actions (ADR 0006/0016/0041).
//!
//! Each handler runs the same pipeline: **authenticate** (the bearer token resolves to
//! a Matrix user), **load** the caller's standing in the guild from the projection,
//! **authorize** with the permission engine, then have the **action bot** perform the
//! substrate write (ADR 0041). The acting user is recorded in the audit log, so actions
//! stay attributable even though the bot performed them.
//!
//! Note the deliberate absence of optimistic writes: the projection is updated by the
//! substrate's AppService echo (ADR 0019), not by these handlers.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use nuck_protocol::api::{
    ActionResponse, BanRequest, ChannelInfo, CreateChannelRequest, CreateGuildRequest,
    CreateRoleRequest, CreateRoleResponse, GuildResponse, GuildView, RoleInfo,
    SetMemberRolesRequest,
};
use nuck_protocol::authorization::EVERYONE_ROLE_ID;
use nuck_protocol::events::{ChannelKind, event_types as et};
use nuck_protocol::permissions::{BOT_LEVEL, Permissions, coarse_power_levels};

use crate::projection::Projection;
use crate::state::AppState;

use super::auth::AuthenticatedUser;
use super::error::ApiErrorResponse;
use super::privileged;

/// `POST /_nuck/v1/guilds/{guildId}/roles` — create a role.
pub async fn create_role(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<Json<CreateRoleResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_ROLES, "MANAGE_ROLES")?;
    if !access.outranks_position(req.position) {
        return Err(ApiErrorResponse::forbidden(
            "cannot create a role at or above your highest role",
        ));
    }
    if !access.can_grant(req.permissions) {
        return Err(ApiErrorResponse::forbidden(
            "you cannot grant a permission you do not have",
        ));
    }

    let bot = privileged::action_bot(&state)?;
    let role_id = privileged::new_role_id();
    let content = serde_json::json!({
        "name": req.name,
        "color": req.color,
        "position": req.position,
        "permissions": req.permissions,
        "hoist": req.hoist,
        "mentionable": req.mentionable,
    });
    let event_id = bot
        .write_state(&guild_id, et::ROLE, &role_id, content)
        .await
        .map_err(privileged::internal)?;

    privileged::record(&state, &guild_id, &actor, "role.create", Some(&role_id), &event_id).await?;
    Ok(Json(CreateRoleResponse { role_id, event_id }))
}

/// `PUT /_nuck/v1/guilds/{guildId}/members/{userId}/roles` — assign a member's roles.
pub async fn set_member_roles(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((guild_id, user_id)): Path<(String, String)>,
    Json(req): Json<SetMemberRolesRequest>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_ROLES, "MANAGE_ROLES")?;

    let target_roles = privileged::member_roles(&state, &guild_id, &user_id).await?;
    if !access.outranks_member(&target_roles) {
        return Err(ApiErrorResponse::forbidden("the target member is not below you"));
    }
    for role in &req.roles {
        if role == EVERYONE_ROLE_ID {
            return Err(ApiErrorResponse::bad_request(
                "`@everyone` is implicit and cannot be assigned",
            ));
        }
        let Some(position) = access.position_of(role) else {
            return Err(ApiErrorResponse::bad_request(format!("unknown role: {role}")));
        };
        if !access.outranks_position(position) {
            return Err(ApiErrorResponse::forbidden(
                "cannot assign a role at or above your highest role",
            ));
        }
        // Handing someone a role hands them its permissions — which is only allowed
        // for permissions you hold yourself (ADR 0020).
        if !access.can_grant(access.role_permissions(role).unwrap_or(Permissions::NONE)) {
            return Err(ApiErrorResponse::forbidden(
                "you cannot grant a permission you do not have",
            ));
        }
    }

    let bot = privileged::action_bot(&state)?;
    let content = serde_json::json!({ "roles": req.roles });
    let event_id = bot
        .write_state(&guild_id, et::MEMBER, &user_id, content)
        .await
        .map_err(privileged::internal)?;

    privileged::record(&state, &guild_id, &actor, "member.roles", Some(&user_id), &event_id).await?;
    Ok(Json(ActionResponse { event_id }))
}

/// `PUT /_nuck/v1/guilds/{guildId}/bans/{userId}` — ban a member (ADR 0016).
pub async fn ban_member(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id: actor, .. }: AuthenticatedUser,
    Path((guild_id, user_id)): Path<(String, String)>,
    Json(req): Json<BanRequest>,
) -> Result<Json<ActionResponse>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &actor).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::BAN_MEMBERS, "BAN_MEMBERS")?;

    let target_roles = privileged::member_roles(&state, &guild_id, &user_id).await?;
    if !access.outranks_member(&target_roles) {
        return Err(ApiErrorResponse::forbidden("the target member is not below you"));
    }

    let bot = privileged::action_bot(&state)?;
    // Two writes: the federated ban record, then the membership removal that enforces
    // it. The state event comes first so the ban federates even if the kick fails; the
    // audit entry is keyed by the state event's id.
    let content = serde_json::json!({
        "reason": req.reason,
        "banned_by": actor,
        "created_at": privileged::now_ms(),
    });
    let event_id = bot
        .write_state(&guild_id, et::BAN, &user_id, content)
        .await
        .map_err(privileged::internal)?;
    bot.ban(&guild_id, &user_id, req.reason.as_deref())
        .await
        .map_err(privileged::internal)?;

    privileged::record(&state, &guild_id, &actor, "ban", Some(&user_id), &event_id).await?;
    Ok(Json(ActionResponse { event_id }))
}

// --- Guild & channel provisioning (MVP M3) --------------------------------

/// `POST /_nuck/v1/guilds` — create a guild: the Space, its baseline roles, and a
/// default text channel, with the projector and bot joined so the projection fills.
pub async fn create_guild(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Json(req): Json<CreateGuildRequest>,
) -> Result<Json<GuildResponse>, ApiErrorResponse> {
    let projection = privileged::projection(&state)?;
    let bot = bot_user(&state)?.to_owned();
    let projector = projector_user(&state)?.to_owned();
    let as_token = appservice_token(&state)?.to_owned();
    let name = req.name.unwrap_or_else(|| "New Guild".to_owned());

    let guild_id = state
        .substrate
        .create_room(
            &token,
            None,
            serde_json::json!({
                "name": name,
                "preset": "private_chat",
                "creation_content": { "type": "m.space" },
                "invite": [projector, bot],
            }),
        )
        .await
        .map_err(privileged::internal)?;

    // Join the AppService users before writing state, so the projector receives it.
    join_appservice(&state, &as_token, &guild_id, &[&projector, &bot]).await;

    // Coarse power levels (ADR 0041): the owner and the bot are the privileged writers.
    send_state(&state, &token, &guild_id, "m.room.power_levels", "", power_levels(&bot)).await?;

    send_state(
        &state,
        &token,
        &guild_id,
        et::GUILD,
        "",
        serde_json::json!({
            "join_mode": "invite_only",
            "directory_visibility": "hidden",
            "default_notification_level": "mentions_only",
        }),
    )
    .await?;

    // The member baseline.
    send_state(
        &state,
        &token,
        &guild_id,
        et::ROLE,
        EVERYONE_ROLE_ID,
        serde_json::json!({
            "name": "@everyone",
            "color": null,
            "position": 0,
            "permissions": Permissions::SEND_MESSAGES
                .union(Permissions::VIEW_CHANNEL)
                .union(Permissions::READ_MESSAGE_HISTORY)
                .union(Permissions::ADD_REACTIONS)
                .union(Permissions::ATTACH_FILES)
                .union(Permissions::EMBED_LINKS),
            "hoist": false,
            "mentionable": false,
        }),
    )
    .await?;

    // The creator's admin role (ADR 0038: creation grants the owner a role).
    send_state(
        &state,
        &token,
        &guild_id,
        et::ROLE,
        "admin",
        serde_json::json!({
            "name": "Admin",
            "color": null,
            "position": 100,
            "permissions": Permissions::ADMINISTRATOR,
            "hoist": true,
            "mentionable": false,
        }),
    )
    .await?;
    send_state(
        &state,
        &token,
        &guild_id,
        et::MEMBER,
        &user_id,
        serde_json::json!({ "roles": ["admin"] }),
    )
    .await?;

    let channel_id = provision_channel(
        &state,
        &token,
        &guild_id,
        ChannelSpec { name: "general", kind: ChannelKind::Text, category: None, position: 0 },
    )
    .await?;

    // Backfill the guild room (ownership + the creator's membership) and the channel.
    sync_room(&state, &guild_id).await?;
    sync_room(&state, &channel_id).await?;
    wait_for_channel(projection, &channel_id).await;
    Ok(Json(GuildResponse { guild_id, channel_id }))
}

/// `GET /_nuck/v1/guilds/{guildId}` — the aggregated guild view from the projection.
pub async fn get_guild(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, .. }: AuthenticatedUser,
    Path(guild_id): Path<String>,
) -> Result<Json<GuildView>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &user_id).await?;
    privileged::require_member(&access)?;

    let projection = privileged::projection(&state)?;
    let channels = projection
        .list_channels(&guild_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|c| ChannelInfo {
            channel_id: c.channel_id,
            kind: c.kind,
            position: c.position,
            category: c.category,
        })
        .collect();
    let roles = projection
        .list_roles(&guild_id)
        .await
        .map_err(privileged::internal)?
        .into_iter()
        .map(|r| {
            let permissions = r.perms();
            RoleInfo {
                role_id: r.role_id,
                name: r.name,
                position: r.position,
                permissions,
                color: r.color.map(|color| color as u32),
                hoist: r.hoist,
                mentionable: r.mentionable,
            }
        })
        .collect();
    let owner = projection
        .get_guild_owner(&guild_id)
        .await
        .map_err(privileged::internal)?;

    let home_server = if access.guild.home_server.is_empty() {
        state.config.server_name.clone()
    } else {
        access.guild.home_server.clone()
    };
    Ok(Json(GuildView {
        guild_id,
        name: access.guild.name,
        home_server,
        owner,
        channels,
        roles,
    }))
}

/// `POST /_nuck/v1/guilds/{guildId}/channels` — create a channel.
pub async fn create_channel(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser { user_id, token }: AuthenticatedUser,
    Path(guild_id): Path<String>,
    Json(req): Json<CreateChannelRequest>,
) -> Result<Json<ChannelInfo>, ApiErrorResponse> {
    let access = privileged::load(&state, &guild_id, &user_id).await?;
    privileged::require_local(&state, &access)?;
    privileged::require_member(&access)?;
    privileged::require(&access, Permissions::MANAGE_CHANNELS, "MANAGE_CHANNELS")?;

    let position = req.position.unwrap_or(0);
    let channel_id = provision_channel(
        &state,
        &token,
        &guild_id,
        ChannelSpec {
            name: &req.name,
            kind: req.kind,
            category: req.category.as_deref(),
            position,
        },
    )
    .await?;

    wait_for_channel(privileged::projection(&state)?, &channel_id).await;
    Ok(Json(ChannelInfo {
        channel_id,
        kind: kind_str(req.kind),
        position,
        category: req.category,
    }))
}

// --- Provisioning helpers ---------------------------------------------------

fn bot_user(state: &AppState) -> Result<&str, ApiErrorResponse> {
    state
        .config
        .bot_user
        .as_deref()
        .ok_or_else(|| ApiErrorResponse::unavailable("the action bot is not configured"))
}

fn projector_user(state: &AppState) -> Result<&str, ApiErrorResponse> {
    state
        .config
        .appservice_user
        .as_deref()
        .ok_or_else(|| ApiErrorResponse::unavailable("the projector is not configured"))
}

fn appservice_token(state: &AppState) -> Result<&str, ApiErrorResponse> {
    state
        .config
        .bot_token
        .as_deref()
        .or(state.config.appservice_as_token.as_deref())
        .ok_or_else(|| ApiErrorResponse::unavailable("the AppService token is not configured"))
}

/// The coarse power levels (ADR 0041) as Matrix `m.room.power_levels` content.
fn power_levels(bot: &str) -> serde_json::Value {
    let levels = coarse_power_levels();
    let mut users = serde_json::Map::new();
    // The room creator is implicitly 100; listing them in `users` is rejected.
    users.insert(bot.to_owned(), serde_json::json!(BOT_LEVEL));
    serde_json::json!({
        "users": users,
        "users_default": levels.users_default,
        "events_default": levels.events_default,
        "state_default": levels.state_default,
        "ban": levels.ban,
        "kick": levels.kick,
        "redact": levels.redact,
        "invite": levels.invite,
    })
}

async fn send_state(
    state: &AppState,
    token: &str,
    room_id: &str,
    event_type: &str,
    state_key: &str,
    content: serde_json::Value,
) -> Result<String, ApiErrorResponse> {
    state
        .substrate
        .send_state_event(token, None, room_id, event_type, state_key, content)
        .await
        .map_err(privileged::internal)
}

async fn join_appservice(state: &AppState, as_token: &str, room_id: &str, users: &[&str]) {
    for user in users {
        if let Err(e) = state.substrate.join_room(as_token, Some(user), room_id).await {
            tracing::warn!(room = %room_id, user = %user, "AppService join failed: {e}");
        }
    }
}

/// Backfill a room's full current state into the projection, read as the projector.
///
/// The AppService push only starts when the projector joins, so state written before
/// that (the `m.room.create` that establishes ownership, the creator's membership) is
/// otherwise missed. This reads resolved state as a member and reconciles it in.
async fn sync_room(state: &AppState, room_id: &str) -> Result<(), ApiErrorResponse> {
    let projector = projector_user(state)?.to_owned();
    let as_token = appservice_token(state)?.to_owned();
    let events = state
        .substrate
        .room_state_as(&as_token, Some(&projector), room_id)
        .await
        .map_err(privileged::internal)?;
    let (ops, ids, _) = crate::projection::reconcile::state_ops(&events);
    privileged::projection(state)?
        .reconcile_room(room_id, &ops, &ids)
        .await
        .map_err(privileged::internal)?;
    Ok(())
}

/// Create a channel room, register it in the guild, and join the AppService users.
struct ChannelSpec<'a> {
    name: &'a str,
    kind: ChannelKind,
    category: Option<&'a str>,
    position: i64,
}

async fn provision_channel(
    state: &AppState,
    token: &str,
    guild_id: &str,
    spec: ChannelSpec<'_>,
) -> Result<String, ApiErrorResponse> {
    let bot = bot_user(state)?.to_owned();
    let projector = projector_user(state)?.to_owned();
    let as_token = appservice_token(state)?.to_owned();
    let server = state.config.server_name.clone();

    let channel_id = state
        .substrate
        .create_room(
            token,
            None,
            serde_json::json!({
                "name": spec.name,
                "preset": "private_chat",
                "invite": [projector, bot],
                "power_level_content_override": power_levels(&bot),
            }),
        )
        .await
        .map_err(privileged::internal)?;

    join_appservice(state, &as_token, &channel_id, &[&projector, &bot]).await;
    sync_room(state, &channel_id).await?;

    send_state(
        state,
        token,
        guild_id,
        et::CHANNEL,
        &channel_id,
        serde_json::json!({
            "kind": spec.kind,
            "position": spec.position,
            "category": spec.category
        }),
    )
    .await?;

    send_state(
        state,
        token,
        guild_id,
        "m.space.child",
        &channel_id,
        serde_json::json!({ "via": [server] }),
    )
    .await?;

    Ok(channel_id)
}

/// Wait (briefly) for the AppService echo to reach the projection, so ids returned to
/// the client are immediately usable by the permission checks.
async fn wait_for_channel(projection: &Projection, channel_id: &str) {
    for _ in 0..50 {
        match projection.get_channel(channel_id).await {
            Ok(Some(_)) => return,
            Ok(None) => tokio::time::sleep(Duration::from_millis(200)).await,
            Err(e) => {
                tracing::warn!(channel = %channel_id, "projection read failed: {e}");
                return;
            }
        }
    }
    tracing::warn!(channel = %channel_id, "projection did not converge before responding");
}

fn kind_str(kind: ChannelKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}
