//! Row types for the derived index (ADR 0019).
//!
//! These mirror the `space.nuck.*` events (see `nuck-protocol`) but flattened for
//! querying. They are **projections**, never a source of truth (ADR 0003).

use nuck_protocol::permissions::Permissions;

/// A guild (Matrix Space).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildRow {
    pub guild_id: String,
    pub home_server: String,
    pub name: Option<String>,
    pub join_mode: Option<String>,
    pub directory_visibility: Option<String>,
    pub default_notification_level: Option<String>,
    pub event_id: String,
}

/// A guild role (`space.nuck.role.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RoleRow {
    pub guild_id: String,
    pub role_id: String,
    pub position: i64,
    /// Permission bitfield, stored as a signed integer.
    pub permissions: i64,
    pub name: Option<String>,
    pub color: Option<i64>,
    pub hoist: bool,
    pub mentionable: bool,
    pub event_id: String,
}

impl RoleRow {
    /// The role's permission set.
    #[must_use]
    pub fn perms(&self) -> Permissions {
        Permissions::from_bits(self.permissions as u64)
    }
}

/// A member's role assignments (`space.nuck.member.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemberRow {
    pub guild_id: String,
    pub user_id: String,
    pub roles: Vec<String>,
    pub event_id: String,
}

/// Room membership, derived from `m.room.member` (ADR 0005/0019).
///
/// Distinct from [`MemberRow`], which holds `space.nuck.member.v1` *role
/// assignments*. This is the substrate-authoritative "who is in this room": guild
/// membership is membership in the guild Space room, channel access is membership in
/// the channel room (ADR 0005/0041).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MembershipRow {
    pub room_id: String,
    pub user_id: String,
    /// `join`, `invite`, `leave`, `ban`, or `knock`.
    pub membership: String,
    pub event_id: String,
}

/// A guild's owner: the creator of the guild space (ADR 0020/0038).
///
/// Derived from `m.room.create` (the `creator` field, or the event's `sender` on room
/// versions that dropped it). Kept separate from [`GuildRow`] because the create event
/// and `space.nuck.guild.v1` are different events.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GuildOwnerRow {
    pub guild_id: String,
    pub owner: String,
    pub event_id: String,
}

/// A channel (`space.nuck.channel.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChannelRow {
    pub channel_id: String,
    pub guild_id: String,
    pub kind: String,
    pub position: i64,
    pub category: Option<String>,
    pub event_id: String,
}

/// A per-channel permission override (`space.nuck.override.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OverrideRow {
    pub channel_id: String,
    /// A role id or a user id.
    pub target: String,
    pub allow: i64,
    pub deny: i64,
    pub event_id: String,
}

/// A guild ban (`space.nuck.ban.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BanRow {
    pub guild_id: String,
    pub user_id: String,
    pub reason: Option<String>,
    pub banned_by: Option<String>,
    pub created_at: Option<i64>,
    pub event_id: String,
}

/// An invite code (`space.nuck.invite.v1`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InviteRow {
    pub guild_id: String,
    pub code_hash: String,
    pub max_uses: Option<i32>,
    pub uses: i32,
    pub expires_at: Option<i64>,
    pub created_by: Option<String>,
    pub event_id: String,
}

/// An append-only audit entry (ADR 0016).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditEntry {
    pub guild_id: String,
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    pub event_id: String,
    pub created_at: i64,
}

/// An audit-log row with its sequence id, for the paginated read (ADR 0016).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditLogRow {
    pub id: i64,
    pub guild_id: String,
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    pub event_id: String,
    pub created_at: i64,
}

/// A searchable message (ADR 0029).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRow {
    pub event_id: String,
    pub room_id: String,
    pub sender: String,
    pub origin_ts: i64,
    pub body: String,
}
