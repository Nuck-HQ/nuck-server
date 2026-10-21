//! Integration tests for the projection pipeline (ADR 0019) against a real Postgres.
//!
//! Skipped unless `TEST_DATABASE_URL` is set, e.g.
//!
//! ```text
//! TEST_DATABASE_URL=postgres://nuck:nuck@127.0.0.1:55432/nuck cargo test -p nuck-server
//! ```

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use nuck_server::api;
use nuck_server::config::Config;
use nuck_server::db;
use nuck_server::matrix::{RawEvent, SubstrateClient};
use nuck_server::permissions::Permissions;
use nuck_server::projection::reconcile::{Reconciler, state_ops};
use nuck_server::projection::{
    AuditEntry, ChannelRow, GuildOwnerRow, GuildRow, MemberRow, MembershipRow, MessageRow,
    OverrideRow, Projection, ProjectionOp, RoleRow,
};
use nuck_server::state::AppState;
use serde_json::json;
use tokio::sync::{Mutex, MutexGuard};
use tower::ServiceExt;

const GUILD: &str = "!guild:example.org";

/// Tests share one database and truncate it, so they must not run concurrently.
fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn setup() -> Option<(Projection, MutexGuard<'static, ()>)> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let guard = lock().lock().await;
    let pool = db::connect(&url).await.expect("connect to TEST_DATABASE_URL");
    db::migrate(&pool).await.expect("run migrations");
    sqlx::query(
        "TRUNCATE guilds, guild_owners, roles, members, memberships, channels, overrides, bans,
                  invites, join_requests, audit_log, messages, processed_events, projection_state",
    )
    .execute(&pool)
    .await
    .expect("truncate");
    Some((Projection::new(pool), guard))
}

fn app_with(projection: Projection) -> Router {
    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        server_name: "example.org".to_owned(),
        database_url: None,
        tuwunel_url: "http://localhost:8008".to_owned(),
        tuwunel_admin_token: None,
        appservice_hs_token: None,
        appservice_user: None,
        appservice_as_token: None,
        bot_user: None,
        bot_token: None,
        appservice_url: None,
        appservice_registration_path: None,
        appservice_id: "nuck-projector".to_owned(),
        reconcile_interval_secs: 300,
        reconcile_on_startup: false,
        message_backfill_limit: None,
        registration_mode: "token".to_owned(),
        registration_token: None,
        admin_users: Vec::new(),
    };
    let substrate = SubstrateClient::from_config(&config);
    api::router(Arc::new(AppState {
        config: Arc::new(config),
        projection: Some(projection),
        substrate,
    }))
}

#[tokio::test]
async fn appservice_transaction_projects_events_end_to_end() {
    let Some((p, _guard)) = setup().await else { return };

    let txn = json!({
        "events": [
            {
                "event_id": "$guild",
                "room_id": GUILD,
                "sender": "@owner:example.org",
                "type": "space.nuck.guild.v1",
                "state_key": "",
                "content": {
                    "join_mode": "invite_only",
                    "directory_visibility": "hidden",
                    "default_notification_level": "mentions_only"
                }
            },
            {
                "event_id": "$role",
                "room_id": GUILD,
                "sender": "@owner:example.org",
                "type": "space.nuck.role.v1",
                "state_key": "mod",
                "content": {
                    "name": "Moderator",
                    "color": null,
                    "position": 50,
                    "permissions": Permissions::MANAGE_MESSAGES.bits(),
                    "hoist": false,
                    "mentionable": true
                }
            },
            {
                "event_id": "$msg",
                "room_id": "!general:example.org",
                "sender": "@u:example.org",
                "type": "m.room.message",
                "origin_server_ts": 1_700_000_000_000u64,
                "content": { "msgtype": "m.text", "body": "hello pineapple" }
            }
        ]
    })
    .to_string();

    let request = || {
        Request::builder()
            .method("PUT")
            .uri("/_matrix/app/v1/transactions/txn1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(txn.clone()))
            .unwrap()
    };

    let response = app_with(p.clone()).oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let roles: (i64,) = sqlx::query_as("SELECT count(*) FROM roles WHERE guild_id = $1")
        .bind(GUILD)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(roles.0, 1, "the role event should have been projected");

    let guilds: (i64,) = sqlx::query_as("SELECT count(*) FROM guilds WHERE guild_id = $1")
        .bind(GUILD)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(guilds.0, 1);

    let hits = p
        .search_messages("pineapple", &["!general:example.org".to_owned()], 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);

    // Replaying the same transaction is a no-op (idempotency, ADR 0019).
    let response = app_with(p.clone()).oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let roles: (i64,) = sqlx::query_as("SELECT count(*) FROM roles WHERE guild_id = $1")
        .bind(GUILD)
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(roles.0, 1, "replayed events must not duplicate");
}

fn guild_row() -> GuildRow {
    GuildRow {
        guild_id: GUILD.to_owned(),
        home_server: "example.org".to_owned(),
        name: Some("Test Guild".to_owned()),
        join_mode: Some("invite_only".to_owned()),
        directory_visibility: Some("hidden".to_owned()),
        default_notification_level: Some("mentions_only".to_owned()),
        event_id: "$guild".to_owned(),
    }
}

#[tokio::test]
async fn record_event_is_idempotent() {
    let Some((p, _guard)) = setup().await else { return };

    assert!(p.record_event("$a", "!room:example.org").await.unwrap());
    assert!(
        !p.record_event("$a", "!room:example.org").await.unwrap(),
        "the second ingestion of the same event must report it as already seen"
    );
}

#[tokio::test]
async fn role_upsert_round_trips_permissions_and_is_idempotent() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    let perms = Permissions::SEND_MESSAGES.union(Permissions::MANAGE_ROLES);
    let mut role = RoleRow {
        guild_id: GUILD.to_owned(),
        role_id: "mod".to_owned(),
        position: 50,
        permissions: perms.bits() as i64,
        name: Some("Moderator".to_owned()),
        color: Some(0x00_ff_00),
        hoist: true,
        mentionable: true,
        event_id: "$role1".to_owned(),
    };

    p.upsert_role(&role).await.unwrap();
    // Re-applying the same event must not change the result.
    p.upsert_role(&role).await.unwrap();

    let stored: RoleRow = sqlx::query_as(
        "SELECT guild_id, role_id, position, permissions, name, color, hoist, mentionable, event_id
         FROM roles WHERE guild_id = $1 AND role_id = $2",
    )
    .bind(GUILD)
    .bind("mod")
    .fetch_one(p.pool())
    .await
    .unwrap();

    assert_eq!(stored.perms(), perms);
    assert_eq!(stored.position, 50);

    // An update via a newer event overwrites the row.
    role.permissions = Permissions::BAN_MEMBERS.bits() as i64;
    role.event_id = "$role2".to_owned();
    p.upsert_role(&role).await.unwrap();

    let stored: RoleRow = sqlx::query_as(
        "SELECT guild_id, role_id, position, permissions, name, color, hoist, mentionable, event_id
         FROM roles WHERE guild_id = $1 AND role_id = $2",
    )
    .bind(GUILD)
    .bind("mod")
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(stored.perms(), Permissions::BAN_MEMBERS);
}

#[tokio::test]
async fn member_roles_round_trip() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    p.upsert_member(&MemberRow {
        guild_id: GUILD.to_owned(),
        user_id: "@u:example.org".to_owned(),
        roles: vec!["mod".to_owned(), "vip".to_owned()],
        event_id: "$m1".to_owned(),
    })
    .await
    .unwrap();

    let stored: MemberRow = sqlx::query_as(
        "SELECT guild_id, user_id, roles, event_id FROM members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(GUILD)
    .bind("@u:example.org")
    .fetch_one(p.pool())
    .await
    .unwrap();

    assert_eq!(stored.roles, vec!["mod".to_owned(), "vip".to_owned()]);
}

#[tokio::test]
async fn membership_round_trips_and_updates() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    p.upsert_membership(&MembershipRow {
        room_id: GUILD.to_owned(),
        user_id: "@u:example.org".to_owned(),
        membership: "join".to_owned(),
        event_id: "$mem1".to_owned(),
    })
    .await
    .unwrap();

    let stored = p.get_membership(GUILD, "@u:example.org").await.unwrap().unwrap();
    assert_eq!(stored.membership, "join");

    // A newer `m.room.member` overwrites the state.
    p.upsert_membership(&MembershipRow {
        room_id: GUILD.to_owned(),
        user_id: "@u:example.org".to_owned(),
        membership: "leave".to_owned(),
        event_id: "$mem2".to_owned(),
    })
    .await
    .unwrap();

    let stored = p.get_membership(GUILD, "@u:example.org").await.unwrap().unwrap();
    assert_eq!(stored.membership, "leave");
    assert_eq!(stored.event_id, "$mem2");
}

#[tokio::test]
async fn guild_owner_round_trips() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    p.upsert_guild_owner(&GuildOwnerRow {
        guild_id: GUILD.to_owned(),
        owner: "@alice:example.org".to_owned(),
        event_id: "$create".to_owned(),
    })
    .await
    .unwrap();

    assert_eq!(
        p.get_guild_owner(GUILD).await.unwrap().as_deref(),
        Some("@alice:example.org")
    );

    // The owner is absent for a guild with no projected `m.room.create`.
    assert_eq!(p.get_guild_owner("!other:example.org").await.unwrap(), None);
}

#[tokio::test]
async fn audit_append_is_idempotent_on_event_id() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    let entry = AuditEntry {
        guild_id: GUILD.to_owned(),
        actor: "@mod:example.org".to_owned(),
        action: "ban".to_owned(),
        target: Some("@spammer:example.org".to_owned()),
        event_id: "$audit1".to_owned(),
        created_at: 1_700_000_000_000,
    };

    p.append_audit(&entry).await.unwrap();
    p.append_audit(&entry).await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log WHERE event_id = $1")
        .bind("$audit1")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(count.0, 1, "replayed audit events must not duplicate");
}

#[tokio::test]
async fn message_search_finds_plaintext_messages_in_scope() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();
    p.upsert_channel(&ChannelRow {
        channel_id: "!general:example.org".to_owned(),
        guild_id: GUILD.to_owned(),
        kind: "text".to_owned(),
        position: 0,
        category: None,
        event_id: "$c1".to_owned(),
    })
    .await
    .unwrap();

    for (id, body) in [
        ("$msg1", "hello world, this is a pineapple"),
        ("$msg2", "unrelated content about bicycles"),
    ] {
        p.upsert_message(&MessageRow {
            event_id: id.to_owned(),
            room_id: "!general:example.org".to_owned(),
            sender: "@u:example.org".to_owned(),
            origin_ts: 1_700_000_000_000,
            body: body.to_owned(),
        })
        .await
        .unwrap();
    }

    let hits = p
        .search_messages("pineapple", &["!general:example.org".to_owned()], 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$msg1");

    // Out-of-scope rooms yield nothing (callers must pass the channels a user may read).
    let hits = p
        .search_messages("pineapple", &["!other:example.org".to_owned()], 10)
        .await
        .unwrap();
    assert!(hits.is_empty());
}

// --- Reconciliation (ADR 0019) ----------------------------------------------

/// A config whose substrate is unreachable (nothing listens on port 1) and whose
/// admin token is set, so the reconciler tries and fails per room.
fn reconcile_config() -> Config {
    Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        server_name: "example.org".to_owned(),
        database_url: None,
        tuwunel_url: "http://127.0.0.1:1".to_owned(),
        tuwunel_admin_token: Some("admin".to_owned()),
        appservice_hs_token: None,
        appservice_user: Some("@nuck-projector:example.org".to_owned()),
        appservice_as_token: None,
        bot_user: None,
        bot_token: None,
        appservice_url: None,
        appservice_registration_path: None,
        appservice_id: "nuck-projector".to_owned(),
        reconcile_interval_secs: 300,
        reconcile_on_startup: false,
        message_backfill_limit: None,
        registration_mode: "token".to_owned(),
        registration_token: None,
        admin_users: Vec::new(),
    }
}

fn state_event(
    room_id: &str,
    event_type: &str,
    state_key: &str,
    event_id: &str,
    content: serde_json::Value,
) -> RawEvent {
    RawEvent {
        event_id: event_id.to_owned(),
        room_id: room_id.to_owned(),
        sender: "@owner:example.org".to_owned(),
        event_type: event_type.to_owned(),
        state_key: Some(state_key.to_owned()),
        origin_server_ts: Some(1_700_000_000_000),
        content,
    }
}

fn role_content(name: &str, position: i64, permissions: Permissions) -> serde_json::Value {
    json!({
        "name": name,
        "color": null,
        "position": position,
        "permissions": permissions.bits(),
        "hoist": false,
        "mentionable": true
    })
}

fn role(event_id: &str, role_id: &str, position: i64, permissions: Permissions) -> RoleRow {
    RoleRow {
        guild_id: GUILD.to_owned(),
        role_id: role_id.to_owned(),
        position,
        permissions: permissions.bits() as i64,
        name: Some(role_id.to_owned()),
        color: None,
        hoist: false,
        mentionable: false,
        event_id: event_id.to_owned(),
    }
}

#[tokio::test]
async fn reconciliation_repairs_drift_and_prunes_removed_state() {
    let Some((p, _guard)) = setup().await else { return };

    // State before drift: a guild with two roles, a member, two channels, an override.
    p.upsert_guild(&guild_row()).await.unwrap();
    p.upsert_role(&role("$r1", "mod", 50, Permissions::SEND_MESSAGES)).await.unwrap();
    p.upsert_role(&role("$r2", "vip", 10, Permissions::NONE)).await.unwrap();
    p.upsert_member(&MemberRow {
        guild_id: GUILD.to_owned(),
        user_id: "@u:example.org".to_owned(),
        roles: vec!["vip".to_owned()],
        event_id: "$m1".to_owned(),
    })
    .await
    .unwrap();
    p.upsert_channel(&ChannelRow {
        channel_id: "!general:example.org".to_owned(),
        guild_id: GUILD.to_owned(),
        kind: "text".to_owned(),
        position: 0,
        category: None,
        event_id: "$c1".to_owned(),
    })
    .await
    .unwrap();
    p.upsert_channel(&ChannelRow {
        channel_id: "!offtopic:example.org".to_owned(),
        guild_id: GUILD.to_owned(),
        kind: "text".to_owned(),
        position: 1,
        category: None,
        event_id: "$c2".to_owned(),
    })
    .await
    .unwrap();
    p.upsert_override(&OverrideRow {
        channel_id: "!general:example.org".to_owned(),
        target: "mod".to_owned(),
        allow: Permissions::SEND_MESSAGES.bits() as i64,
        deny: 0,
        event_id: "$o1".to_owned(),
    })
    .await
    .unwrap();

    // A stale membership whose `m.room.member` event is gone from resolved state.
    p.upsert_membership(&MembershipRow {
        room_id: GUILD.to_owned(),
        user_id: "@u:example.org".to_owned(),
        membership: "join".to_owned(),
        event_id: "$mem-old".to_owned(),
    })
    .await
    .unwrap();

    // A stale owner whose `m.room.create` is gone from resolved state.
    p.upsert_guild_owner(&GuildOwnerRow {
        guild_id: GUILD.to_owned(),
        owner: "@old:example.org".to_owned(),
        event_id: "$create-old".to_owned(),
    })
    .await
    .unwrap();

    // Append-only projections that reconciliation must never touch.
    p.upsert_message(&MessageRow {        event_id: "$msg1".to_owned(),
        room_id: "!general:example.org".to_owned(),
        sender: "@u:example.org".to_owned(),
        origin_ts: 1,
        body: "keep me".to_owned(),
    })
    .await
    .unwrap();
    p.append_audit(&AuditEntry {
        guild_id: GUILD.to_owned(),
        actor: "@mod:example.org".to_owned(),
        action: "ban".to_owned(),
        target: None,
        event_id: "$audit1".to_owned(),
        created_at: 1,
    })
    .await
    .unwrap();

    // Resolved state after drift: `mod` changed, `vip` deleted, `admin` added,
    // `!offtopic` deleted, the member's roles changed, `!general` moved.
    let guild_state = vec![
        state_event(
            GUILD,
            "space.nuck.guild.v1",
            "",
            "$guild",
            json!({
                "join_mode": "invite_only",
                "directory_visibility": "hidden",
                "default_notification_level": "mentions_only"
            }),
        ),
        state_event(
            GUILD,
            "space.nuck.role.v1",
            "mod",
            "$r1b",
            role_content("Moderator", 50, Permissions::MANAGE_MESSAGES),
        ),
        state_event(
            GUILD,
            "space.nuck.role.v1",
            "admin",
            "$r3",
            role_content("Admin", 100, Permissions::ADMINISTRATOR),
        ),
        state_event(GUILD, "space.nuck.member.v1", "@u:example.org", "$m2", json!({ "roles": [] })),
        state_event(
            GUILD,
            "space.nuck.channel.v1",
            "!general:example.org",
            "$c1b",
            json!({ "kind": "text", "position": 5, "category": null }),
        ),
    ];
    let (ops, ids, is_guild) = state_ops(&guild_state);
    assert!(is_guild, "a guild event makes this a guild room");
    p.reconcile_room(GUILD, &ops, &ids).await.unwrap();

    // The channel's override drifted too: one new override replaces the old one.
    let channel_state = vec![state_event(
        "!general:example.org",
        "space.nuck.override.v1",
        "mod",
        "$o2",
        json!({ "allow": 0, "deny": Permissions::SEND_MESSAGES.bits() }),
    )];
    let (ops, ids, is_guild) = state_ops(&channel_state);
    assert!(!is_guild);
    p.reconcile_room("!general:example.org", &ops, &ids).await.unwrap();

    // Updates applied.
    let mod_role: RoleRow = sqlx::query_as(
        "SELECT guild_id, role_id, position, permissions, name, color, hoist, mentionable, event_id
         FROM roles WHERE guild_id = $1 AND role_id = $2",
    )
    .bind(GUILD)
    .bind("mod")
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(mod_role.perms(), Permissions::MANAGE_MESSAGES);
    assert_eq!(mod_role.event_id, "$r1b", "the newer state event must win");

    let roles: Vec<(String,)> =
        sqlx::query_as("SELECT role_id FROM roles WHERE guild_id = $1 ORDER BY role_id")
            .bind(GUILD)
            .fetch_all(p.pool())
            .await
            .unwrap();
    assert_eq!(
        roles.into_iter().map(|(id,)| id).collect::<Vec<_>>(),
        vec!["admin".to_owned(), "mod".to_owned()],
        "the removed `vip` role must be pruned and `admin` added"
    );

    let member: MemberRow = sqlx::query_as(
        "SELECT guild_id, user_id, roles, event_id FROM members WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(GUILD)
    .bind("@u:example.org")
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert!(member.roles.is_empty());

    // Removed channel pruned, surviving channel reflects the newer event.
    let channels: Vec<ChannelRow> = sqlx::query_as(
        "SELECT channel_id, guild_id, kind, position, category, event_id
         FROM channels WHERE guild_id = $1 ORDER BY channel_id",
    )
    .bind(GUILD)
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].channel_id, "!general:example.org");
    assert_eq!(channels[0].position, 5);
    assert_eq!(channels[0].event_id, "$c1b");

    let override_row: OverrideRow = sqlx::query_as(
        "SELECT channel_id, target, allow, deny, event_id
         FROM overrides WHERE channel_id = $1 AND target = $2",
    )
    .bind("!general:example.org")
    .bind("mod")
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(override_row.deny, Permissions::SEND_MESSAGES.bits() as i64);
    assert_eq!(override_row.event_id, "$o2");

    // The stale membership is pruned.
    assert!(
        p.get_membership(GUILD, "@u:example.org").await.unwrap().is_none(),
        "a membership whose m.room.member event is gone must be pruned"
    );
    // The stale owner is pruned.
    assert!(
        p.get_guild_owner(GUILD).await.unwrap().is_none(),
        "an owner whose m.room.create event is gone must be pruned"
    );

    // Append-only projections survive reconciliation.
    let messages: (i64,) = sqlx::query_as("SELECT count(*) FROM messages WHERE event_id = $1")
        .bind("$msg1")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(messages.0, 1, "reconciliation must not touch message search");
    let audit: (i64,) = sqlx::query_as("SELECT count(*) FROM audit_log WHERE event_id = $1")
        .bind("$audit1")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(audit.0, 1, "reconciliation must not touch the audit log");

    // A second pass over the same resolved state changes nothing.
    let before: (i64,) = sqlx::query_as("SELECT count(*) FROM roles")
        .fetch_one(p.pool())
        .await
        .unwrap();
    let (ops, ids, _) = state_ops(&guild_state);
    p.reconcile_room(GUILD, &ops, &ids).await.unwrap();
    let after: (i64,) = sqlx::query_as("SELECT count(*) FROM roles")
        .fetch_one(p.pool())
        .await
        .unwrap();
    assert_eq!(before, after, "reconciliation is idempotent");
}

#[tokio::test]
async fn reconciliation_survives_a_substrate_outage() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();

    let reconciler = Reconciler::new(
        p.clone(),
        SubstrateClient::from_config(&reconcile_config()),
        Some("@nuck-projector:example.org".to_owned()),
        Duration::from_secs(300),
        None,
    );

    let report = reconciler
        .reconcile_once(false)
        .await
        .expect("a substrate outage must not fail the pass");

    assert_eq!(report.rooms, 0);
    assert!(report.failures >= 1, "the unreachable guild is counted, then logged");
}

#[tokio::test]
async fn removing_a_channel_deletes_it_and_its_overrides() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();
    let channel = "!chan:example.org";
    p.upsert_channel(&ChannelRow {
        channel_id: channel.to_owned(),
        guild_id: GUILD.to_owned(),
        kind: "text".to_owned(),
        position: 0,
        category: None,
        event_id: "$chan".to_owned(),
    })
    .await
    .unwrap();
    p.upsert_override(&OverrideRow {
        channel_id: channel.to_owned(),
        target: "everyone".to_owned(),
        allow: 0,
        deny: 0,
        event_id: "$ov".to_owned(),
    })
    .await
    .unwrap();
    assert!(p.get_channel(channel).await.unwrap().is_some());

    p.apply(ProjectionOp::RemoveChannel { channel_id: channel.to_owned() })
        .await
        .unwrap();
    assert!(p.get_channel(channel).await.unwrap().is_none(), "channel removed");
    assert!(p.list_overrides(channel).await.unwrap().is_empty(), "overrides removed");
}

#[tokio::test]
async fn removing_a_role_deletes_overrides_that_targeted_it() {
    let Some((p, _guard)) = setup().await else { return };
    p.upsert_guild(&guild_row()).await.unwrap();
    p.upsert_role(&role("$r1", "mod", 10, Permissions::NONE)).await.unwrap();
    p.upsert_override(&OverrideRow {
        channel_id: "!chan:example.org".to_owned(),
        target: "mod".to_owned(),
        allow: 0,
        deny: 0,
        event_id: "$ov".to_owned(),
    })
    .await
    .unwrap();

    p.apply(ProjectionOp::RemoveRole { guild_id: GUILD.to_owned(), role_id: "mod".to_owned() })
        .await
        .unwrap();
    assert!(p.list_roles(GUILD).await.unwrap().is_empty(), "role removed");
    assert!(
        p.list_overrides("!chan:example.org").await.unwrap().is_empty(),
        "its override was removed"
    );
}
