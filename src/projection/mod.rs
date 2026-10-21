//! The federated-event → Postgres projection pipeline (ADR 0019).
//!
//! Ingestion is an **AppService push** to a trusted projector user auto-joined to
//! every guild room, plus a **reconciliation loop** that re-reads room state on
//! startup and periodically (via `GET /_synapse/admin/v1/rooms/{room_id}/state`).
//!
//! Every write here is an **idempotent upsert**; the store is rebuildable (ADR 0003).
//! [`Projection::record_event`] is the idempotency guard: it returns `false` if the
//! event was already ingested, so callers can skip re-projecting it.
//!
//! Two projection kinds:
//! - *current-state* — guild settings, roles, channel structure, memberships — written
//!   by the `upsert_*` methods from the substrate's resolved room state; and
//! - *append-only* — audit log, message search — written by `append_audit` /
//!   `upsert_message`, keyed by event id.
//!
//! [`reconcile`] is the self-healing half: it re-reads resolved room state on startup
//! and periodically and repairs drift, which is what keeps ADR 0003's "rebuildable
//! index" property honest.

pub mod ingest;
mod models;
pub mod reconcile;

use anyhow::Result;
use sqlx::PgPool;

pub use ingest::{RawEvent, Transaction};
pub use models::*;

/// A single projection operation produced from a federated event (ADR 0019).
///
/// The `Remove*` variants encode **deletion**. Matrix has no state deletion, so a
/// removed entity is written as a state event with **empty content** — the same shape a
/// redaction produces, and the same convention `m.room.member` already uses (a redacted
/// member event is ignored, not rejected). This keeps the store rebuildable: current
/// state carries the empty event, and replaying it re-applies the removal.
#[derive(Debug, Clone)]
pub enum ProjectionOp {
    Guild(GuildRow),
    GuildOwner(GuildOwnerRow),
    Role(RoleRow),
    Member(MemberRow),
    Membership(MembershipRow),
    Channel(ChannelRow),
    Override(OverrideRow),
    Ban(BanRow),
    Invite(InviteRow),
    JoinRequest(JoinRequestRow),
    Message(MessageRow),
    RemoveGuild(String),
    RemoveRole { guild_id: String, role_id: String },
    RemoveMember { guild_id: String, user_id: String },
    RemoveChannel { channel_id: String },
    RemoveBan { guild_id: String, user_id: String },
    RemoveInvite { guild_id: String, code_hash: String },
    RemoveJoinRequest { guild_id: String, user_id: String },
}

/// The projection: an idempotent, rebuildable index over federated state (ADR 0019).
#[derive(Clone)]
pub struct Projection {
    pool: PgPool,
}

impl Projection {
    /// Wrap a Postgres pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Apply a projection operation.
    pub async fn apply(&self, op: ProjectionOp) -> Result<()> {
        match op {
            ProjectionOp::Guild(r) => self.upsert_guild(&r).await,
            ProjectionOp::GuildOwner(r) => self.upsert_guild_owner(&r).await,
            ProjectionOp::Role(r) => self.upsert_role(&r).await,
            ProjectionOp::Member(r) => self.upsert_member(&r).await,
            ProjectionOp::Membership(r) => self.upsert_membership(&r).await,
            ProjectionOp::Channel(r) => self.upsert_channel(&r).await,
            ProjectionOp::Override(r) => self.upsert_override(&r).await,
            ProjectionOp::Ban(r) => self.upsert_ban(&r).await,
            ProjectionOp::Invite(r) => self.upsert_invite(&r).await,
            ProjectionOp::JoinRequest(r) => self.upsert_join_request(&r).await,
            ProjectionOp::Message(r) => self.upsert_message(&r).await,
            ProjectionOp::RemoveGuild(guild_id) => self.delete_guild(&guild_id).await,
            ProjectionOp::RemoveRole { guild_id, role_id } => {
                self.delete_role(&guild_id, &role_id).await
            }
            ProjectionOp::RemoveMember { guild_id, user_id } => {
                self.delete_member(&guild_id, &user_id).await
            }
            ProjectionOp::RemoveChannel { channel_id } => self.delete_channel(&channel_id).await,
            ProjectionOp::RemoveBan { guild_id, user_id } => {
                self.delete_ban(&guild_id, &user_id).await
            }
            ProjectionOp::RemoveInvite { guild_id, code_hash } => {
                self.delete_invite(&guild_id, &code_hash).await
            }
            ProjectionOp::RemoveJoinRequest { guild_id, user_id } => {
                self.delete_join_request(&guild_id, &user_id).await
            }
        }
    }

    /// Record an ingested event. Returns `true` if it was **newly** seen.
    ///
    /// The idempotency guard for the whole pipeline: a `false` return means the event
    /// has already been projected and can be skipped (ADR 0019).
    pub async fn record_event(&self, event_id: &str, room_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO processed_events (event_id, room_id) VALUES ($1, $2)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(event_id)
        .bind(room_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Upsert a guild.
    pub async fn upsert_guild(&self, g: &GuildRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO guilds
                (guild_id, home_server, name, join_mode, directory_visibility,
                 default_notification_level, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, now())
             ON CONFLICT (guild_id) DO UPDATE SET
                home_server = EXCLUDED.home_server,
                name = EXCLUDED.name,
                join_mode = EXCLUDED.join_mode,
                directory_visibility = EXCLUDED.directory_visibility,
                default_notification_level = EXCLUDED.default_notification_level,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&g.guild_id)
        .bind(&g.home_server)
        .bind(&g.name)
        .bind(&g.join_mode)
        .bind(&g.directory_visibility)
        .bind(&g.default_notification_level)
        .bind(&g.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert a guild's owner (from `m.room.create`).
    pub async fn upsert_guild_owner(&self, o: &GuildOwnerRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO guild_owners (guild_id, owner, event_id, updated_at)
             VALUES ($1, $2, $3, now())
             ON CONFLICT (guild_id) DO UPDATE SET
                owner = EXCLUDED.owner,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&o.guild_id)
        .bind(&o.owner)
        .bind(&o.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A guild's owner, if projected (ADR 0020).
    pub async fn get_guild_owner(&self, guild_id: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT owner FROM guild_owners WHERE guild_id = $1")
                .bind(guild_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(owner,)| owner))
    }

    /// Upsert a role.
    pub async fn upsert_role(&self, r: &RoleRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO roles
                (guild_id, role_id, position, permissions, name, color, hoist, mentionable, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now())
             ON CONFLICT (guild_id, role_id) DO UPDATE SET
                position = EXCLUDED.position,
                permissions = EXCLUDED.permissions,
                name = EXCLUDED.name,
                color = EXCLUDED.color,
                hoist = EXCLUDED.hoist,
                mentionable = EXCLUDED.mentionable,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&r.guild_id)
        .bind(&r.role_id)
        .bind(r.position)
        .bind(r.permissions)
        .bind(&r.name)
        .bind(r.color)
        .bind(r.hoist)
        .bind(r.mentionable)
        .bind(&r.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a role, and any override that targeted it (ADR 0020).
    pub async fn delete_role(&self, guild_id: &str, role_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM roles WHERE guild_id = $1 AND role_id = $2")
            .bind(guild_id)
            .bind(role_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM overrides WHERE target = $1")
            .bind(role_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a guild and its owner record (a removed guild).
    pub async fn delete_guild(&self, guild_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM guilds WHERE guild_id = $1")
            .bind(guild_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM guild_owners WHERE guild_id = $1")
            .bind(guild_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a member's role assignments (e.g. on kick or leave).
    pub async fn delete_member(&self, guild_id: &str, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM members WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a channel and its overrides.
    pub async fn delete_channel(&self, channel_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM channels WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM overrides WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete a ban (unban).
    pub async fn delete_ban(&self, guild_id: &str, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM bans WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Upsert a member's role assignments.
    pub async fn upsert_member(&self, m: &MemberRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO members (guild_id, user_id, roles, event_id, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (guild_id, user_id) DO UPDATE SET
                roles = EXCLUDED.roles,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&m.guild_id)
        .bind(&m.user_id)
        .bind(&m.roles)
        .bind(&m.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert a user's room membership (ADR 0005).
    pub async fn upsert_membership(&self, m: &MembershipRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO memberships (room_id, user_id, membership, event_id, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (room_id, user_id) DO UPDATE SET
                membership = EXCLUDED.membership,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&m.room_id)
        .bind(&m.user_id)
        .bind(&m.membership)
        .bind(&m.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A user's membership in a room, if projected.
    pub async fn get_membership(
        &self,
        room_id: &str,
        user_id: &str,
    ) -> Result<Option<MembershipRow>> {
        let row = sqlx::query_as::<_, MembershipRow>(
            "SELECT room_id, user_id, membership, event_id
             FROM memberships WHERE room_id = $1 AND user_id = $2",
        )
        .bind(room_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Upsert a channel.
    pub async fn upsert_channel(&self, c: &ChannelRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO channels (channel_id, guild_id, kind, position, category, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (channel_id) DO UPDATE SET
                guild_id = EXCLUDED.guild_id,
                kind = EXCLUDED.kind,
                position = EXCLUDED.position,
                category = EXCLUDED.category,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&c.channel_id)
        .bind(&c.guild_id)
        .bind(&c.kind)
        .bind(c.position)
        .bind(&c.category)
        .bind(&c.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert a per-channel override.
    pub async fn upsert_override(&self, o: &OverrideRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO overrides (channel_id, target, allow, deny, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, now())
             ON CONFLICT (channel_id, target) DO UPDATE SET
                allow = EXCLUDED.allow,
                deny = EXCLUDED.deny,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&o.channel_id)
        .bind(&o.target)
        .bind(o.allow)
        .bind(o.deny)
        .bind(&o.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert a ban.
    pub async fn upsert_ban(&self, b: &BanRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO bans (guild_id, user_id, reason, banned_by, created_at, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (guild_id, user_id) DO UPDATE SET
                reason = EXCLUDED.reason,
                banned_by = EXCLUDED.banned_by,
                created_at = EXCLUDED.created_at,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&b.guild_id)
        .bind(&b.user_id)
        .bind(&b.reason)
        .bind(&b.banned_by)
        .bind(b.created_at)
        .bind(&b.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert an invite.
    pub async fn upsert_invite(&self, i: &InviteRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO invites
                (guild_id, code_hash, code, max_uses, uses, expires_at, created_by, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
             ON CONFLICT (guild_id, code_hash) DO UPDATE SET
                code = COALESCE(EXCLUDED.code, invites.code),
                max_uses = EXCLUDED.max_uses,
                uses = EXCLUDED.uses,
                expires_at = EXCLUDED.expires_at,
                created_by = EXCLUDED.created_by,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&i.guild_id)
        .bind(&i.code_hash)
        .bind(&i.code)
        .bind(i.max_uses)
        .bind(i.uses)
        .bind(i.expires_at)
        .bind(&i.created_by)
        .bind(&i.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A guild's invites.
    pub async fn list_invites(&self, guild_id: &str) -> Result<Vec<InviteRow>> {
        let rows = sqlx::query_as::<_, InviteRow>(
            "SELECT guild_id, code_hash, code, max_uses, uses, expires_at, created_by, event_id
             FROM invites WHERE guild_id = $1
             ORDER BY code_hash ASC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Look up an invite by its **code hash** across all guilds (redeem/preview).
    pub async fn get_invite_by_hash(&self, code_hash: &str) -> Result<Option<InviteRow>> {
        let row = sqlx::query_as::<_, InviteRow>(
            "SELECT guild_id, code_hash, code, max_uses, uses, expires_at, created_by, event_id
             FROM invites WHERE code_hash = $1",
        )
        .bind(code_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Delete an invite (revoke).
    pub async fn delete_invite(&self, guild_id: &str, code_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM invites WHERE guild_id = $1 AND code_hash = $2")
            .bind(guild_id)
            .bind(code_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Upsert a join request.
    pub async fn upsert_join_request(&self, r: &JoinRequestRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO join_requests
                (guild_id, user_id, message, status, requested_at, event_id, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (guild_id, user_id) DO UPDATE SET
                message = EXCLUDED.message,
                status = EXCLUDED.status,
                requested_at = EXCLUDED.requested_at,
                event_id = EXCLUDED.event_id,
                updated_at = now()",
        )
        .bind(&r.guild_id)
        .bind(&r.user_id)
        .bind(&r.message)
        .bind(&r.status)
        .bind(r.requested_at)
        .bind(&r.event_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a join request.
    pub async fn delete_join_request(&self, guild_id: &str, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM join_requests WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A guild's join requests, oldest first.
    pub async fn list_join_requests(&self, guild_id: &str) -> Result<Vec<JoinRequestRow>> {
        let rows = sqlx::query_as::<_, JoinRequestRow>(
            "SELECT guild_id, user_id, message, status, requested_at, event_id
             FROM join_requests WHERE guild_id = $1
             ORDER BY requested_at ASC, user_id ASC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// One join request, if any.
    pub async fn get_join_request(
        &self,
        guild_id: &str,
        user_id: &str,
    ) -> Result<Option<JoinRequestRow>> {
        let row = sqlx::query_as::<_, JoinRequestRow>(
            "SELECT guild_id, user_id, message, status, requested_at, event_id
             FROM join_requests WHERE guild_id = $1 AND user_id = $2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Whether a user is banned from a guild.
    pub async fn get_ban(&self, guild_id: &str, user_id: &str) -> Result<Option<BanRow>> {
        let row = sqlx::query_as::<_, BanRow>(
            "SELECT guild_id, user_id, reason, banned_by, created_at, event_id
             FROM bans WHERE guild_id = $1 AND user_id = $2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// The number of joined members in a guild.
    pub async fn count_guild_members(&self, guild_id: &str) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM memberships WHERE room_id = $1 AND membership = 'join'",
        )
        .bind(guild_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// Record a pending registration request (ADR 0037).
    pub async fn insert_registration_request(&self, r: &RegistrationRequestRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO registration_requests (id, username, user_id, created_at, status)
             VALUES ($1, $2, $3, $4, 'pending')",
        )
        .bind(&r.id)
        .bind(&r.username)
        .bind(&r.user_id)
        .bind(r.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Pending (`status = 'pending'`) registration requests, oldest first (ADR 0037).
    pub async fn list_registration_requests(&self) -> Result<Vec<RegistrationRequestRow>> {
        let rows = sqlx::query_as::<_, RegistrationRequestRow>(
            "SELECT id, username, user_id, created_at, status FROM registration_requests
             WHERE status = 'pending'
             ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// One registration request, if any.
    pub async fn get_registration_request(
        &self,
        id: &str,
    ) -> Result<Option<RegistrationRequestRow>> {
        let row = sqlx::query_as::<_, RegistrationRequestRow>(
            "SELECT id, username, user_id, created_at, status FROM registration_requests
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Delete a registration request (approval — lifts the login block).
    pub async fn delete_registration_request(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM registration_requests WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Mark a registration request denied (leaves the queue, keeps the login block).
    pub async fn deny_registration_request(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE registration_requests SET status = 'denied' WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Whether a user is held back from login by an unresolved registration request
    /// (`approval` mode, ADR 0046).
    pub async fn registration_blocked(&self, user_id: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM registration_requests WHERE user_id = $1)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    // --- Friends & blocking (ADR 0030) -------------------------------------

    /// Record a friendship (normalised so `user_a < user_b`). Idempotent.
    pub async fn insert_friendship(&self, a: &str, b: &str, since: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO friendships (user_a, user_b, since)
             VALUES (LEAST($1, $2), GREATEST($1, $2), $3)
             ON CONFLICT (user_a, user_b) DO NOTHING",
        )
        .bind(a)
        .bind(b)
        .bind(since)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove a friendship.
    pub async fn delete_friendship(&self, a: &str, b: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM friendships
             WHERE user_a = LEAST($1, $2) AND user_b = GREATEST($1, $2)",
        )
        .bind(a)
        .bind(b)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Whether two users are friends.
    pub async fn are_friends(&self, a: &str, b: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM friendships
             WHERE user_a = LEAST($1, $2) AND user_b = GREATEST($1, $2))",
        )
        .bind(a)
        .bind(b)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// A user's friends as `(other_user, since)`, ordered by user id.
    pub async fn list_friends(&self, user_id: &str) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query_as(
            "SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END AS other, since
             FROM friendships WHERE user_a = $1 OR user_b = $1
             ORDER BY other ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Record a block. Idempotent.
    pub async fn insert_block(&self, blocker: &str, blocked: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO blocks (blocker, blocked) VALUES ($1, $2)
             ON CONFLICT (blocker, blocked) DO NOTHING",
        )
        .bind(blocker)
        .bind(blocked)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove a block.
    pub async fn delete_block(&self, blocker: &str, blocked: &str) -> Result<()> {
        sqlx::query("DELETE FROM blocks WHERE blocker = $1 AND blocked = $2")
            .bind(blocker)
            .bind(blocked)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A user's block list, ordered by user id.
    pub async fn list_blocks(&self, blocker: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT blocked FROM blocks WHERE blocker = $1 ORDER BY blocked ASC")
                .bind(blocker)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(user,)| user).collect())
    }

    /// Whether either user has blocked the other.
    pub async fn is_blocked_either(&self, a: &str, b: &str) -> Result<bool> {
        let row: (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM blocks
             WHERE (blocker = $1 AND blocked = $2) OR (blocker = $2 AND blocked = $1))",
        )
        .bind(a)
        .bind(b)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// Record a pending friend request. Idempotent on the pair.
    pub async fn insert_friend_request(
        &self,
        from_user: &str,
        to_user: &str,
        message: Option<&str>,
        created_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO friend_requests (from_user, to_user, message, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (from_user, to_user) DO NOTHING",
        )
        .bind(from_user)
        .bind(to_user)
        .bind(message)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A pending request in a given direction, if any.
    pub async fn get_friend_request(
        &self,
        from_user: &str,
        to_user: &str,
    ) -> Result<Option<(Option<String>, i64)>> {
        let row = sqlx::query_as(
            "SELECT message, created_at FROM friend_requests
             WHERE from_user = $1 AND to_user = $2",
        )
        .bind(from_user)
        .bind(to_user)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Delete a pending request in a given direction.
    pub async fn delete_friend_request(&self, from_user: &str, to_user: &str) -> Result<()> {
        sqlx::query("DELETE FROM friend_requests WHERE from_user = $1 AND to_user = $2")
            .bind(from_user)
            .bind(to_user)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete pending requests in both directions between two users.
    pub async fn delete_friend_requests_between(&self, a: &str, b: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM friend_requests
             WHERE (from_user = $1 AND to_user = $2) OR (from_user = $2 AND to_user = $1)",
        )
        .bind(a)
        .bind(b)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A user's pending friend requests as `(from, to, message, created_at)`.
    pub async fn list_friend_requests(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String, Option<String>, i64)>> {
        let rows = sqlx::query_as(
            "SELECT from_user, to_user, message, created_at FROM friend_requests
             WHERE from_user = $1 OR to_user = $1
             ORDER BY created_at ASC, from_user ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Append an audit entry (idempotent on `event_id`).
    pub async fn append_audit(&self, a: &AuditEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_log (guild_id, actor, action, target, event_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(&a.guild_id)
        .bind(&a.actor)
        .bind(&a.action)
        .bind(&a.target)
        .bind(&a.event_id)
        .bind(a.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Index a searchable message (idempotent on `event_id`).
    pub async fn upsert_message(&self, m: &MessageRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO messages (event_id, room_id, sender, origin_ts, body)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (event_id) DO UPDATE SET body = EXCLUDED.body",
        )
        .bind(&m.event_id)
        .bind(&m.room_id)
        .bind(&m.sender)
        .bind(m.origin_ts)
        .bind(&m.body)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Full-text search over plaintext guild channels (ADR 0029).
    ///
    /// **Permission filtering is not applied here** — callers must restrict `room_ids`
    /// to channels the user can read (ADR 0029).
    pub async fn search_messages(
        &self,
        query: &str,
        room_ids: &[String],
        limit: i64,
    ) -> Result<Vec<MessageRow>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT event_id, room_id, sender, origin_ts, body
             FROM messages
             WHERE room_id = ANY($1)
               AND search @@ plainto_tsquery('simple', $2)
             ORDER BY origin_ts DESC
             LIMIT $3",
        )
        .bind(room_ids)
        .bind(query)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Reconcile one room from its **full current state** (ADR 0019).
    ///
    /// Applies every operation, then **prunes** any row scoped to the room whose
    /// writing event is no longer part of current state — this is what repairs drift
    /// after a missed or out-of-order push, and what makes the store rebuildable.
    ///
    /// Deliberately not transactional: every step is an idempotent upsert or a scoped
    /// delete, so a crash mid-pass leaves a state the next pass repairs.
    ///
    /// Returns the number of operations applied.
    pub async fn reconcile_room(
        &self,
        room_id: &str,
        ops: &[ProjectionOp],
        current_event_ids: &[String],
    ) -> Result<usize> {
        for op in ops {
            self.apply(op.clone()).await?;
        }
        self.prune_room(room_id, current_event_ids).await?;
        Ok(ops.len())
    }

    /// Delete rows scoped to `room_id` whose writing event is not in current state.
    ///
    /// Each table is scoped by the column its rows key the room on: guild-level rows by
    /// `guild_id`, overrides by `channel_id`. Tables that hold nothing for this room
    /// are simply no-ops, so a single pass covers both guild and channel rooms.
    ///
    /// Written as literal statements (not built from strings) so the SQL is auditable
    /// and cannot be a dynamic-string injection site.
    async fn prune_room(&self, room_id: &str, current_event_ids: &[String]) -> Result<()> {
        const PURGES: &[&str] = &[
            "DELETE FROM guilds WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM roles WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM members WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM bans WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM invites WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM join_requests WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM channels WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM overrides WHERE channel_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM memberships WHERE room_id = $1 AND event_id <> ALL($2::text[])",
            "DELETE FROM guild_owners WHERE guild_id = $1 AND event_id <> ALL($2::text[])",
        ];
        for sql in PURGES {
            sqlx::query(*sql)
                .bind(room_id)
                .bind(current_event_ids)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// The guild rooms known to the projection (reconciliation seeds).
    pub async fn guild_ids(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT guild_id FROM guilds")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// The channel rooms known to the projection (reconciliation seeds).
    pub async fn channel_ids(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT channel_id FROM channels")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// Read one guild, if the projection knows it.
    pub async fn get_guild(&self, guild_id: &str) -> Result<Option<GuildRow>> {
        let row = sqlx::query_as::<_, GuildRow>(
            "SELECT guild_id, home_server, name, join_mode, directory_visibility,
                    default_notification_level, event_id
             FROM guilds WHERE guild_id = $1",
        )
        .bind(guild_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Every role defined in a guild, highest position first.
    pub async fn list_roles(&self, guild_id: &str) -> Result<Vec<RoleRow>> {
        let rows = sqlx::query_as::<_, RoleRow>(
            "SELECT guild_id, role_id, position, permissions, name, color, hoist, mentionable, event_id
             FROM roles WHERE guild_id = $1
             ORDER BY position DESC, role_id ASC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// A member's role assignments, if any are recorded.
    pub async fn get_member(&self, guild_id: &str, user_id: &str) -> Result<Option<MemberRow>> {
        let row = sqlx::query_as::<_, MemberRow>(
            "SELECT guild_id, user_id, roles, event_id
             FROM members WHERE guild_id = $1 AND user_id = $2",
        )
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Read one channel, if the projection knows it.
    pub async fn get_channel(&self, channel_id: &str) -> Result<Option<ChannelRow>> {
        let row = sqlx::query_as::<_, ChannelRow>(
            "SELECT channel_id, guild_id, kind, position, category, event_id
             FROM channels WHERE channel_id = $1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// A guild's channels, in display order.
    pub async fn list_channels(&self, guild_id: &str) -> Result<Vec<ChannelRow>> {
        let rows = sqlx::query_as::<_, ChannelRow>(
            "SELECT channel_id, guild_id, kind, position, category, event_id
             FROM channels WHERE guild_id = $1
             ORDER BY position ASC, channel_id ASC",
        )
        .bind(guild_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The guilds a user is a **joined** member of, ordered by guild id.
    ///
    /// Guild membership is membership in the guild Space room (ADR 0005), so this joins
    /// `guilds` to `memberships`. Returns `(guild_id, home_server, name, owner)`.
    pub async fn list_guilds_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String, Option<String>, Option<String>)>> {
        let rows = sqlx::query_as(
            "SELECT g.guild_id, g.home_server, g.name, o.owner
             FROM guilds g
             JOIN memberships m ON m.room_id = g.guild_id
             LEFT JOIN guild_owners o ON o.guild_id = g.guild_id
             WHERE m.user_id = $1 AND m.membership = 'join'
             ORDER BY g.guild_id ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// A guild's members (joined or invited), ordered by user id, cursor-paginated.
    ///
    /// Returns `(user_id, membership, roles)`. Display names are not projected.
    pub async fn list_guild_members(
        &self,
        guild_id: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<(String, String, Vec<String>)>> {
        let rows = sqlx::query_as(
            "SELECT m.user_id, m.membership, COALESCE(mb.roles, '{}'::text[])
             FROM memberships m
             LEFT JOIN members mb ON mb.guild_id = m.room_id AND mb.user_id = m.user_id
             WHERE m.room_id = $1
               AND m.membership IN ('join', 'invite')
               AND ($2::text IS NULL OR m.user_id > $2)
             ORDER BY m.user_id ASC
             LIMIT $3",
        )
        .bind(guild_id)
        .bind(after)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// A guild's bans, ordered by user id, cursor-paginated.
    pub async fn list_bans(
        &self,
        guild_id: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<BanRow>> {
        let rows = sqlx::query_as::<_, BanRow>(
            "SELECT guild_id, user_id, reason, banned_by, created_at, event_id
             FROM bans WHERE guild_id = $1
               AND ($2::text IS NULL OR user_id > $2)
             ORDER BY user_id ASC
             LIMIT $3",
        )
        .bind(guild_id)
        .bind(after)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// A guild's audit log, newest first, cursor-paginated by descending id (ADR 0016).
    pub async fn list_audit(
        &self,
        guild_id: &str,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<AuditLogRow>> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT id, guild_id, actor, action, target, event_id, created_at
             FROM audit_log WHERE guild_id = $1
               AND ($2::bigint IS NULL OR id < $2)
             ORDER BY id DESC
             LIMIT $3",
        )
        .bind(guild_id)
        .bind(before_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// A channel's permission overrides (ADR 0020).
    pub async fn list_overrides(&self, channel_id: &str) -> Result<Vec<OverrideRow>> {
        let rows = sqlx::query_as::<_, OverrideRow>(
            "SELECT channel_id, target, allow, deny, event_id
             FROM overrides WHERE channel_id = $1
             ORDER BY target ASC",
        )
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Read a projection bookkeeping value.
    pub async fn get_state(&self, key: &str) -> Result<Option<String>> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM projection_state WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(value,)| value))
    }

    /// Write a projection bookkeeping value.
    pub async fn set_state(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO projection_state (key, value, updated_at) VALUES ($1, $2, now())
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
