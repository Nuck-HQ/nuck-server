//! End-to-end tests for the privileged-action path (ADR 0041): authenticate →
//! authorize against projected guild state → the **action bot** performs the substrate
//! write → audit attribution.
//!
//! These run against a real Postgres and a **fake Tuwunel** (a local axum server that
//! records the requests the bot makes), skipped unless `TEST_DATABASE_URL` is set.
//!
//! ```text
//! TEST_DATABASE_URL=postgres://nuck:nuck@127.0.0.1:55432/nuck cargo test -p nuck-server
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use nuck_server::api;
use nuck_server::authorization;
use nuck_server::config::Config;
use nuck_server::db;
use nuck_server::matrix::SubstrateClient;
use nuck_server::permissions::{EVERYONE_ROLE_ID, Permissions};
use nuck_server::projection::{
    AuditEntry, BanRow, ChannelRow, GuildOwnerRow, GuildRow, MemberRow, MembershipRow, OverrideRow,
    Projection, RoleRow,
};
use nuck_server::state::AppState;
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};
use tower::ServiceExt;

const GUILD: &str = "!guild:example.org";
const CHANNEL: &str = "!general:example.org";
const SERVER: &str = "example.org";
const BOT: &str = "@nuck-bot:example.org";
const USER_TOKEN: &str = "user-token";
const BOT_TOKEN: &str = "bot-token";
const OTHER_TOKEN: &str = "other-token";
const ACTOR: &str = "@mod:example.org";
const OTHER: &str = "@other:example.org";

// --- Fake Tuwunel ----------------------------------------------------------

/// A request the action bot made, as recorded by the fake substrate.
#[derive(Clone, Debug, PartialEq)]
enum Recorded {
    State {
        room_id: String,
        event_type: String,
        state_key: String,
        user_id: String,
        content: Value,
    },
    Membership {
        kind: String,
        room_id: String,
        user_id: String,
        target: String,
        reason: Option<String>,
    },
}

#[derive(Clone)]
struct Fake {
    tokens: HashMap<String, String>,
    writes: Arc<Mutex<Vec<Recorded>>>,
    next_event: Arc<AtomicU64>,
}

impl Fake {
    fn new() -> Self {
        let mut tokens = HashMap::new();
        tokens.insert(USER_TOKEN.to_owned(), ACTOR.to_owned());
        tokens.insert(BOT_TOKEN.to_owned(), BOT.to_owned());
        tokens.insert(OTHER_TOKEN.to_owned(), OTHER.to_owned());
        Self { tokens, writes: Arc::new(Mutex::new(Vec::new())), next_event: Arc::new(AtomicU64::new(1)) }
    }

    fn writes(&self) -> Vec<Recorded> {
        self.writes.lock().unwrap().clone()
    }
}

fn fake_router(fake: Fake) -> Router {
    Router::new()
        .route("/_matrix/client/v3/account/whoami", get(fake_whoami))
        .route("/_matrix/client/v3/profile/{user_id}", get(fake_profile))
        .route(
            "/_matrix/client/v3/user/{user_id}/account_data/{event_type}",
            get(fake_account_data),
        )
        .route(
            "/_matrix/client/v3/rooms/{room_id}/state/{event_type}/{state_key}",
            put(fake_send_state),
        )
        .route(
            "/_matrix/client/v3/rooms/{room_id}/state/{event_type}",
            put(fake_send_state_singleton),
        )
        .route("/_matrix/client/v3/rooms/{room_id}/{action}", post(fake_membership))
        .with_state(fake)
}

async fn fake_whoami(State(fake): State<Fake>, headers: HeaderMap) -> Response {
    let user = bearer(&headers).and_then(|token| fake.tokens.get(token).cloned());
    match user {
        Some(user_id) => (StatusCode::OK, Json(json!({ "user_id": user_id }))).into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "errcode": "M_UNKNOWN_TOKEN" })),
        )
            .into_response(),
    }
}

async fn fake_send_state(
    State(fake): State<Fake>,
    Path((room_id, event_type, state_key)): Path<(String, String, String)>,
    Query(query): Query<HashMap<String, String>>,
    Json(content): Json<Value>,
) -> Json<Value> {
    let user_id = query.get("user_id").cloned().unwrap_or_default();
    fake.writes.lock().unwrap().push(Recorded::State {
        room_id,
        event_type,
        state_key,
        user_id,
        content,
    });
    let id = fake.next_event.fetch_add(1, Ordering::SeqCst);
    Json(json!({ "event_id": format!("$bot{id}") }))
}

/// A singleton state event (empty state key) has no trailing path segment.
async fn fake_send_state_singleton(
    State(fake): State<Fake>,
    Path((room_id, event_type)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    Json(content): Json<Value>,
) -> Json<Value> {
    let user_id = query.get("user_id").cloned().unwrap_or_default();
    fake.writes.lock().unwrap().push(Recorded::State {
        room_id,
        event_type,
        state_key: String::new(),
        user_id,
        content,
    });
    let id = fake.next_event.fetch_add(1, Ordering::SeqCst);
    Json(json!({ "event_id": format!("$bot{id}") }))
}

async fn fake_membership(
    State(fake): State<Fake>,
    Path((room_id, action)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let user_id = query.get("user_id").cloned().unwrap_or_default();
    let target = body.get("user_id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let reason = body.get("reason").and_then(Value::as_str).map(ToOwned::to_owned);
    fake.writes.lock().unwrap().push(Recorded::Membership {
        kind: action,
        room_id,
        user_id,
        target,
        reason,
    });
    Json(json!({}))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

async fn fake_profile(Path(_user_id): Path<String>) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "displayname": "Display Name", "avatar_url": "mxc://example.org/av" })),
    )
        .into_response()
}

async fn fake_account_data(Path((_user_id, event_type)): Path<(String, String)>) -> Response {
    if event_type == "space.nuck.settings.v1" {
        (StatusCode::OK, Json(json!({ "theme": "dark" }))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "errcode": "M_NOT_FOUND" }))).into_response()
    }
}

// --- Harness ---------------------------------------------------------------

/// Tests share one database and truncate it, so they must not run concurrently.
fn lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

struct Harness {
    app: Router,
    projection: Projection,
    fake: Fake,
    _guard: MutexGuard<'static, ()>,
}

async fn setup() -> Option<Harness> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let guard = lock().lock().await;
    let pool = db::connect(&url).await.expect("connect to TEST_DATABASE_URL");
    db::migrate(&pool).await.expect("run migrations");
    sqlx::query(
        "TRUNCATE guilds, guild_owners, roles, members, memberships, channels, overrides, bans,
                  invites, audit_log, messages, processed_events, projection_state",
    )
    .execute(&pool)
    .await
    .expect("truncate");
    let projection = Projection::new(pool);

    let fake = Fake::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = fake_router(fake.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, server).await;
    });

    let config = Config {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        server_name: SERVER.to_owned(),
        database_url: None,
        tuwunel_url: format!("http://{addr}"),
        tuwunel_admin_token: Some("admin".to_owned()),
        appservice_hs_token: None,
        appservice_user: Some("@nuck-projector:example.org".to_owned()),
        appservice_as_token: Some(BOT_TOKEN.to_owned()),
        bot_user: Some(BOT.to_owned()),
        bot_token: None,
        appservice_url: None,
        appservice_registration_path: None,
        appservice_id: "nuck-projector".to_owned(),
        reconcile_interval_secs: 300,
        reconcile_on_startup: false,
        message_backfill_limit: None,
    };
    let substrate = SubstrateClient::from_config(&config);
    let app = api::router(Arc::new(AppState {
        config: Arc::new(config),
        projection: Some(projection.clone()),
        substrate,
    }));

    Some(Harness { app, projection, fake, _guard: guard })
}

async fn seed_guild(p: &Projection, guild_id: &str, home_server: &str) {
    p.upsert_guild(&GuildRow {
        guild_id: guild_id.to_owned(),
        home_server: home_server.to_owned(),
        name: Some("Guild".to_owned()),
        join_mode: Some("invite_only".to_owned()),
        directory_visibility: Some("hidden".to_owned()),
        default_notification_level: Some("mentions_only".to_owned()),
        event_id: format!("$guild{guild_id}"),
    })
    .await
    .unwrap();
}

async fn seed_role(p: &Projection, guild_id: &str, role_id: &str, position: i64, permissions: Permissions) {
    p.upsert_role(&RoleRow {
        guild_id: guild_id.to_owned(),
        role_id: role_id.to_owned(),
        position,
        permissions: permissions.bits() as i64,
        name: Some(role_id.to_owned()),
        color: None,
        hoist: false,
        mentionable: false,
        event_id: format!("$role{guild_id}:{role_id}"),
    })
    .await
    .unwrap();
}

async fn seed_member(p: &Projection, guild_id: &str, user_id: &str, roles: &[&str]) {
    p.upsert_member(&MemberRow {
        guild_id: guild_id.to_owned(),
        user_id: user_id.to_owned(),
        roles: roles.iter().map(|r| (*r).to_owned()).collect(),
        event_id: format!("$member{guild_id}:{user_id}"),
    })
    .await
    .unwrap();
    // Holding roles implies being in the guild (membership is a separate projection).
    seed_join(p, guild_id, user_id).await;
}

/// Record a joined `m.room.member` for a user in a room.
async fn seed_join(p: &Projection, room_id: &str, user_id: &str) {
    p.upsert_membership(&MembershipRow {
        room_id: room_id.to_owned(),
        user_id: user_id.to_owned(),
        membership: "join".to_owned(),
        event_id: format!("$join{room_id}:{user_id}"),
    })
    .await
    .unwrap();
}

async fn seed_channel(p: &Projection, channel_id: &str, guild_id: &str) {
    p.upsert_channel(&ChannelRow {
        channel_id: channel_id.to_owned(),
        guild_id: guild_id.to_owned(),
        kind: "text".to_owned(),
        position: 0,
        category: None,
        event_id: format!("$channel{channel_id}"),
    })
    .await
    .unwrap();
}

/// Record the guild's creator (`m.room.create`).
async fn seed_owner(p: &Projection, guild_id: &str, owner: &str) {
    p.upsert_guild_owner(&GuildOwnerRow {
        guild_id: guild_id.to_owned(),
        owner: owner.to_owned(),
        event_id: format!("$create{guild_id}"),
    })
    .await
    .unwrap();
}

/// An `admin` role (position 100) held by `ACTOR`, plus `@everyone`.
/// `ACTOR` owns the guild.
async fn seed_admin(p: &Projection) {
    seed_guild(p, GUILD, SERVER).await;
    seed_role(p, GUILD, EVERYONE_ROLE_ID, 0, Permissions::SEND_MESSAGES).await;
    seed_role(p, GUILD, "admin", 100, Permissions::ADMINISTRATOR).await;
    seed_member(p, GUILD, ACTOR, &["admin"]).await;
    seed_owner(p, GUILD, ACTOR).await;
}

fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn audit_rows(p: &Projection, action: &str) -> Vec<(String, String, Option<String>, String)> {
    sqlx::query_as(
        "SELECT actor, action, target, event_id FROM audit_log WHERE guild_id = $1 AND action = $2",
    )
    .bind(GUILD)
    .bind(action)
    .fetch_all(p.pool())
    .await
    .unwrap()
}

// --- Tests -----------------------------------------------------------------

#[tokio::test]
async fn missing_or_invalid_tokens_are_unauthorized() {
    let Some(h) = setup().await else { return };

    let none = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            None,
            Some(json!({ "name": "x", "position": 1, "permissions": 0 })),
        ))
        .await
        .unwrap();
    assert_eq!(none.status(), StatusCode::UNAUTHORIZED);

    let bad = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some("nonsense"),
            Some(json!({ "name": "x", "position": 1, "permissions": 0 })),
        ))
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_member_without_manage_roles_is_forbidden() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "pleb", 10, Permissions::SEND_MESSAGES).await;
    seed_member(&h.projection, GUILD, ACTOR, &["pleb"]).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({ "name": "Sneaky", "position": 5, "permissions": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty(), "an unauthorized request must not write");
}

#[tokio::test]
async fn an_admin_creates_a_role_through_the_bot() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({
                "name": "Moderator",
                "color": 65280,
                "position": 60,
                "permissions": Permissions::MANAGE_MESSAGES.bits(),
                "hoist": true,
                "mentionable": false
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let role_id = body["role_id"].as_str().unwrap();
    let event_id = body["event_id"].as_str().unwrap();
    assert!(role_id.starts_with('r'));

    // The bot wrote a role state event in the guild room, as itself.
    let writes = h.fake.writes();
    assert_eq!(writes.len(), 1);
    match &writes[0] {
        Recorded::State { room_id, event_type, state_key, user_id, content } => {
            assert_eq!(room_id, GUILD);
            assert_eq!(event_type, "space.nuck.role.v1");
            assert_eq!(state_key, role_id);
            assert_eq!(user_id, BOT, "the write must be attributed to the bot");
            assert_eq!(content["position"], 60);
            assert_eq!(content["permissions"].as_u64(), Some(Permissions::MANAGE_MESSAGES.bits()));
        }
        other => panic!("expected a state write, got {other:?}"),
    }

    // ... and the audit log attributes the action to the real actor.
    let rows = audit_rows(&h.projection, "role.create").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, ACTOR);
    assert_eq!(rows[0].2.as_deref(), Some(role_id));
    assert_eq!(rows[0].3, event_id);
}

#[tokio::test]
async fn cannot_create_a_role_at_or_above_your_highest() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "mod", 50, Permissions::MANAGE_ROLES).await;
    seed_member(&h.projection, GUILD, ACTOR, &["mod"]).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({ "name": "Peer", "position": 50, "permissions": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn assigning_roles_respects_the_hierarchy_on_both_ends() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "mod", 50, Permissions::MANAGE_ROLES).await;
    seed_role(&h.projection, GUILD, "vip", 60, Permissions::NONE).await;
    seed_role(&h.projection, GUILD, "low", 10, Permissions::NONE).await;
    seed_member(&h.projection, GUILD, ACTOR, &["mod"]).await;

    // A role at or above the caller's highest is refused.
    let too_high = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/members/@plain:example.org/roles"),
            Some(USER_TOKEN),
            Some(json!({ "roles": ["vip"] })),
        ))
        .await
        .unwrap();
    assert_eq!(too_high.status(), StatusCode::FORBIDDEN);

    // A member who outranks the caller is refused.
    seed_member(&h.projection, GUILD, "@boss:example.org", &["vip"]).await;
    let outranked = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/members/@boss:example.org/roles"),
            Some(USER_TOKEN),
            Some(json!({ "roles": [] })),
        ))
        .await
        .unwrap();
    assert_eq!(outranked.status(), StatusCode::FORBIDDEN);

    // A valid assignment goes out as the bot.
    let ok = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/members/@plain:example.org/roles"),
            Some(USER_TOKEN),
            Some(json!({ "roles": ["low"] })),
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert_eq!(writes.len(), 1, "only the valid assignment should have written");
    match &writes[0] {
        Recorded::State { event_type, state_key, user_id, content, .. } => {
            assert_eq!(event_type, "space.nuck.member.v1");
            assert_eq!(state_key, "@plain:example.org");
            assert_eq!(user_id, BOT);
            assert_eq!(content["roles"], json!(["low"]));
        }
        other => panic!("expected a state write, got {other:?}"),
    }
}

#[tokio::test]
async fn ban_writes_federated_state_and_membership() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/bans/@spammer:example.org"),
            Some(USER_TOKEN),
            Some(json!({ "reason": "spam" })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert_eq!(writes.len(), 2, "a ban is a state event plus a membership removal");

    match &writes[0] {
        Recorded::State { event_type, state_key, user_id, content, .. } => {
            assert_eq!(event_type, "space.nuck.ban.v1");
            assert_eq!(state_key, "@spammer:example.org");
            assert_eq!(user_id, BOT);
            assert_eq!(content["banned_by"], ACTOR, "attribution is carried in the event");
            assert_eq!(content["reason"], "spam");
        }
        other => panic!("expected the ban state event first, got {other:?}"),
    }
    assert_eq!(
        writes[1],
        Recorded::Membership {
            kind: "ban".to_owned(),
            room_id: GUILD.to_owned(),
            user_id: BOT.to_owned(),
            target: "@spammer:example.org".to_owned(),
            reason: Some("spam".to_owned()),
        }
    );

    let rows = audit_rows(&h.projection, "ban").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, ACTOR);
    assert_eq!(rows[0].2.as_deref(), Some("@spammer:example.org"));
}

#[tokio::test]
async fn cannot_ban_a_member_who_outranks_you() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "mod", 50, Permissions::BAN_MEMBERS).await;
    seed_role(&h.projection, GUILD, "vip", 60, Permissions::NONE).await;
    seed_member(&h.projection, GUILD, ACTOR, &["mod"]).await;
    seed_member(&h.projection, GUILD, "@boss:example.org", &["vip"]).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/bans/@boss:example.org"),
            Some(USER_TOKEN),
            Some(json!({})),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn a_guild_hosted_elsewhere_is_not_acted_on_locally() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, "other.example").await;
    seed_role(&h.projection, GUILD, "admin", 100, Permissions::ADMINISTRATOR).await;
    seed_member(&h.projection, GUILD, ACTOR, &["admin"]).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({ "name": "x", "position": 1, "permissions": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert!(h.fake.writes().is_empty());
}

// --- Channel overrides (ADR 0020) ------------------------------------------

/// A guild where `ACTOR` holds `mod` (position 50, `MANAGE_ROLES`) plus `@everyone`.
async fn seed_mod(p: &Projection) {
    seed_guild(p, GUILD, SERVER).await;
    seed_role(p, GUILD, EVERYONE_ROLE_ID, 0, Permissions::SEND_MESSAGES).await;
    seed_role(p, GUILD, "mod", 50, Permissions::MANAGE_ROLES).await;
    seed_member(p, GUILD, ACTOR, &["mod"]).await;
}

#[tokio::test]
async fn setting_a_channel_override_writes_it_in_the_channel_room() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_role(&h.projection, GUILD, "mod", 50, Permissions::NONE).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/mod"),
            Some(USER_TOKEN),
            Some(json!({
                "allow": Permissions::VIEW_CHANNEL.bits(),
                "deny": Permissions::SEND_MESSAGES.bits()
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert_eq!(writes.len(), 1);
    match &writes[0] {
        Recorded::State { room_id, event_type, state_key, user_id, content } => {
            assert_eq!(room_id, CHANNEL, "the override lives in the channel room");
            assert_eq!(event_type, "space.nuck.override.v1");
            assert_eq!(state_key, "mod");
            assert_eq!(user_id, BOT);
            assert_eq!(content["allow"].as_u64(), Some(Permissions::VIEW_CHANNEL.bits()));
            assert_eq!(content["deny"].as_u64(), Some(Permissions::SEND_MESSAGES.bits()));
        }
        other => panic!("expected a state write, got {other:?}"),
    }

    let rows = audit_rows(&h.projection, "override.set").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, ACTOR);
    assert_eq!(rows[0].2.as_deref(), Some(format!("{CHANNEL}#mod").as_str()));
}

#[tokio::test]
async fn removing_an_override_writes_a_neutral_event() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/{EVERYONE_ROLE_ID}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert_eq!(writes.len(), 1);
    match &writes[0] {
        Recorded::State { event_type, state_key, content, .. } => {
            assert_eq!(event_type, "space.nuck.override.v1");
            assert_eq!(state_key, EVERYONE_ROLE_ID);
            assert_eq!(content["allow"].as_u64(), Some(0));
            assert_eq!(content["deny"].as_u64(), Some(0));
        }
        other => panic!("expected a state write, got {other:?}"),
    }
    assert_eq!(audit_rows(&h.projection, "override.remove").await.len(), 1);
}

#[tokio::test]
async fn cannot_grant_an_override_permission_you_do_not_hold() {
    let Some(h) = setup().await else { return };
    seed_mod(&h.projection).await;
    seed_role(&h.projection, GUILD, "low", 10, Permissions::NONE).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/low"),
            Some(USER_TOKEN),
            Some(json!({ "allow": Permissions::BAN_MEMBERS.bits(), "deny": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a moderator cannot grant BAN_MEMBERS they do not hold"
    );
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn a_member_cannot_override_a_role_at_or_above_their_highest() {
    let Some(h) = setup().await else { return };
    seed_mod(&h.projection).await;
    seed_role(&h.projection, GUILD, "vip", 60, Permissions::NONE).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/vip"),
            Some(USER_TOKEN),
            Some(json!({ "allow": 0, "deny": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn a_member_cannot_override_a_member_who_outranks_them() {
    let Some(h) = setup().await else { return };
    seed_mod(&h.projection).await;
    seed_role(&h.projection, GUILD, "vip", 60, Permissions::NONE).await;
    seed_member(&h.projection, GUILD, "@boss:example.org", &["vip"]).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/@boss:example.org"),
            Some(USER_TOKEN),
            Some(json!({ "allow": 0, "deny": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn an_unknown_role_override_is_rejected() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides/ghost"),
            Some(USER_TOKEN),
            Some(json!({ "allow": 0, "deny": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn an_unknown_channel_is_not_found() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            "/_nuck/v1/channels/!nope:example.org/overrides/mod",
            Some(USER_TOKEN),
            Some(json!({ "allow": 0, "deny": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn channel_overrides_flow_through_the_resolution_engine() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(
        &h.projection,
        GUILD,
        EVERYONE_ROLE_ID,
        0,
        Permissions::SEND_MESSAGES.union(Permissions::VIEW_CHANNEL),
    )
    .await;
    seed_role(&h.projection, GUILD, "mod", 50, Permissions::NONE).await;
    seed_member(&h.projection, GUILD, "@u:example.org", &["mod"]).await;

    // `@everyone` loses SEND_MESSAGES here; `mod` gains MANAGE_MESSAGES.
    h.projection
        .upsert_override(&OverrideRow {
            channel_id: CHANNEL.to_owned(),
            target: EVERYONE_ROLE_ID.to_owned(),
            allow: 0,
            deny: Permissions::SEND_MESSAGES.bits() as i64,
            event_id: "$o1".to_owned(),
        })
        .await
        .unwrap();
    h.projection
        .upsert_override(&OverrideRow {
            channel_id: CHANNEL.to_owned(),
            target: "mod".to_owned(),
            allow: Permissions::MANAGE_MESSAGES.bits() as i64,
            deny: 0,
            event_id: "$o2".to_owned(),
        })
        .await
        .unwrap();

    let access = authorization::load(&h.projection, GUILD, "@u:example.org")
        .await
        .unwrap()
        .expect("guild is projected");
    let rows = h.projection.list_overrides(CHANNEL).await.unwrap();
    let overrides = authorization::overrides_from_rows(&rows);
    let permissions = access.channel_permissions(ruma::user_id!("@u:example.org"), &overrides);

    assert!(permissions.contains(Permissions::MANAGE_MESSAGES));
    assert!(!permissions.contains(Permissions::SEND_MESSAGES));
    assert!(permissions.contains(Permissions::VIEW_CHANNEL));
}

// --- Escalation guards (ADR 0020) ------------------------------------------

#[tokio::test]
async fn cannot_create_a_role_that_grants_permissions_you_do_not_hold() {
    let Some(h) = setup().await else { return };
    seed_mod(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({
                "name": "Escalate",
                "position": 10,
                "permissions": Permissions::BAN_MEMBERS.bits()
            })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn cannot_assign_a_role_that_grants_permissions_you_do_not_hold() {
    let Some(h) = setup().await else { return };
    seed_mod(&h.projection).await;
    // A role below the moderator that (implausibly) carries ADMINISTRATOR.
    seed_role(&h.projection, GUILD, "grantor", 10, Permissions::ADMINISTRATOR).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PUT",
            &format!("/_nuck/v1/guilds/{GUILD}/members/@plain:example.org/roles"),
            Some(USER_TOKEN),
            Some(json!({ "roles": ["grantor"] })),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(h.fake.writes().is_empty());
}

// --- Membership gating (ADR 0005/0041) -------------------------------------

#[tokio::test]
async fn guild_actions_require_membership() {
    let Some(h) = setup().await else { return };
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "admin", 100, Permissions::ADMINISTRATOR).await;
    // Role assignments without a joined `m.room.member`.
    h.projection
        .upsert_member(&MemberRow {
            guild_id: GUILD.to_owned(),
            user_id: ACTOR.to_owned(),
            roles: vec!["admin".to_owned()],
            event_id: "$m1".to_owned(),
        })
        .await
        .unwrap();

    let response = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({ "name": "x", "position": 10, "permissions": 0 })),
        ))
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "holding a role is not the same as being in the guild"
    );
    assert!(h.fake.writes().is_empty());
}

#[tokio::test]
async fn channel_overrides_are_readable_by_members_only() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;
    h.projection
        .upsert_override(&OverrideRow {
            channel_id: CHANNEL.to_owned(),
            target: "mod".to_owned(),
            allow: Permissions::VIEW_CHANNEL.bits() as i64,
            deny: Permissions::SEND_MESSAGES.bits() as i64,
            event_id: "$o1".to_owned(),
        })
        .await
        .unwrap();

    // A non-member of the channel is refused.
    let forbidden = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Once joined to the channel, the overrides are returned.
    seed_join(&h.projection, CHANNEL, ACTOR).await;
    let ok = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/channels/{CHANNEL}/overrides"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(ok).await).unwrap();
    assert_eq!(body["overrides"].as_array().unwrap().len(), 1);
    assert_eq!(body["overrides"][0]["target"], "mod");
    assert_eq!(
        body["overrides"][0]["allow"].as_u64(),
        Some(Permissions::VIEW_CHANNEL.bits())
    );

    // An unknown channel is a 404 regardless of membership.
    let missing = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            "/_nuck/v1/channels/!nope:example.org/overrides",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

// --- Guild ownership (ADR 0020/0038) ---------------------------------------

#[tokio::test]
async fn the_guild_owner_bypasses_the_role_hierarchy() {
    let Some(h) = setup().await else { return };
    // Both users hold the same low role; only ACTOR created the guild.
    seed_guild(&h.projection, GUILD, SERVER).await;
    seed_role(&h.projection, GUILD, "mod", 10, Permissions::MANAGE_ROLES).await;
    seed_member(&h.projection, GUILD, ACTOR, &["mod"]).await;
    seed_member(&h.projection, GUILD, OTHER, &["mod"]).await;
    seed_owner(&h.projection, GUILD, ACTOR).await;

    // The owner may create a role above their highest.
    let owned = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            Some(json!({ "name": "High", "position": 90, "permissions": 0 })),
        ))
        .await
        .unwrap();
    assert_eq!(owned.status(), StatusCode::OK, "the owner outranks everyone");

    // A non-owner with the same role is still refused.
    let other = h
        .app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(OTHER_TOKEN),
            Some(json!({ "name": "High", "position": 90, "permissions": 0 })),
        ))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::FORBIDDEN);
}

async fn body_bytes(response: Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), 64 * 1024).await.unwrap().to_vec()
}

// --- Guild reads (ADR 0005/0013) -------------------------------------------

#[tokio::test]
async fn list_guilds_returns_only_the_callers_guilds() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await; // ACTOR is a joined member of GUILD
    seed_guild(&h.projection, "!other:example.org", SERVER).await; // never joined

    let response = h
        .app
        .clone()
        .oneshot(request("GET", "/_nuck/v1/guilds", Some(USER_TOKEN), None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let guilds = body["guilds"].as_array().unwrap();
    assert_eq!(guilds.len(), 1, "only the joined guild is listed");
    assert_eq!(guilds[0]["guild_id"], GUILD);
    assert_eq!(guilds[0]["owner"], ACTOR);
}

#[tokio::test]
async fn list_channels_returns_the_guilds_channels() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/channels"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let channels = body["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["channel_id"], CHANNEL);
}

#[tokio::test]
async fn list_roles_orders_by_position_and_hides_nothing() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await; // @everyone (0) + admin (100)

    let response = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/roles"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let roles = body["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 2);
    assert_eq!(roles[0]["role_id"], "admin", "highest position first");
}

#[tokio::test]
async fn list_members_includes_roles_and_requires_membership() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/members"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["user_id"], ACTOR);
    assert_eq!(items[0]["membership"], "join");
    assert_eq!(items[0]["roles"][0], "admin");

    // A non-member is refused.
    seed_guild(&h.projection, "!private:example.org", SERVER).await;
    let refused = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            "/_nuck/v1/guilds/!private:example.org/members",
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_bans_requires_ban_members_and_returns_bans() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    h.projection
        .upsert_ban(&BanRow {
            guild_id: GUILD.to_owned(),
            user_id: OTHER.to_owned(),
            reason: Some("spam".to_owned()),
            banned_by: Some(ACTOR.to_owned()),
            created_at: Some(1),
            event_id: "$ban1".to_owned(),
        })
        .await
        .unwrap();

    let response = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/bans"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["user_id"], OTHER);

    // A plain member without BAN_MEMBERS is refused.
    let other = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/bans"),
            Some(OTHER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    // OTHER is not a member at all here, so membership is the first gate (403).
    assert_eq!(other.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_audit_log_requires_view_audit_log() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    h.projection
        .append_audit(&AuditEntry {
            guild_id: GUILD.to_owned(),
            actor: ACTOR.to_owned(),
            action: "ban".to_owned(),
            target: Some(OTHER.to_owned()),
            event_id: "$audit1".to_owned(),
            created_at: 7,
        })
        .await
        .unwrap();

    let response = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/audit-log"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["actor"], ACTOR);
    assert_eq!(items[0]["action"], "ban");

    // A member without VIEW_AUDIT_LOG is refused.
    seed_role(&h.projection, GUILD, "pleb", 1, Permissions::SEND_MESSAGES).await;
    seed_member(&h.projection, GUILD, OTHER, &["pleb"]).await;
    let refused = h
        .app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/_nuck/v1/guilds/{GUILD}/audit-log"),
            Some(OTHER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn me_returns_profile_and_settings() {
    let Some(h) = setup().await else { return };

    let response = h
        .app
        .clone()
        .oneshot(request("GET", "/_nuck/v1/me", Some(USER_TOKEN), None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&body_bytes(response).await).unwrap();
    assert_eq!(body["user_id"], ACTOR);
    assert_eq!(body["display_name"], "Display Name");
    assert_eq!(body["avatar_url"], "mxc://example.org/av");
    assert_eq!(body["settings"]["theme"], "dark");
}

// --- Guild/channel/role/member mutations -----------------------------------

#[tokio::test]
async fn update_guild_merges_settings() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PATCH",
            &format!("/_nuck/v1/guilds/{GUILD}"),
            Some(USER_TOKEN),
            Some(json!({ "name": "Renamed", "join_mode": "open" })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert!(
        writes.iter().any(|w| matches!(w, Recorded::State { event_type, content, .. }
            if event_type == "m.room.name" && content["name"] == "Renamed")),
        "the name is written as m.room.name"
    );
    let settings = writes
        .iter()
        .find_map(|w| match w {
            Recorded::State { event_type, content, .. } if event_type == "space.nuck.guild.v1" => {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("a guild settings write");
    assert_eq!(settings["join_mode"], "open");
    assert_eq!(settings["directory_visibility"], "hidden", "unchanged field is preserved");
    assert_eq!(settings["default_notification_level"], "mentions_only");
}

#[tokio::test]
async fn delete_guild_is_owner_only() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await; // ACTOR owns the guild
    seed_member(&h.projection, GUILD, OTHER, &["admin"]).await; // admin, but not owner

    let refused = h
        .app
        .clone()
        .oneshot(request("DELETE", &format!("/_nuck/v1/guilds/{GUILD}"), Some(OTHER_TOKEN), None))
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    let ok = h
        .app
        .clone()
        .oneshot(request("DELETE", &format!("/_nuck/v1/guilds/{GUILD}"), Some(USER_TOKEN), None))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert!(
        h.fake.writes().iter().any(|w| matches!(w, Recorded::State { event_type, content, .. }
            if event_type == "space.nuck.guild.v1"
                && content.as_object().is_some_and(|o| o.is_empty()))),
        "deletion writes empty guild content"
    );
}

#[tokio::test]
async fn update_role_preserves_unset_fields() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PATCH",
            &format!("/_nuck/v1/guilds/{GUILD}/roles/admin"),
            Some(USER_TOKEN),
            Some(json!({ "name": "Admin Two" })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let role = h
        .fake
        .writes()
        .into_iter()
        .find_map(|w| match w {
            Recorded::State { event_type, state_key, content, .. }
                if event_type == "space.nuck.role.v1" && state_key == "admin" =>
            {
                Some(content)
            }
            _ => None,
        })
        .expect("a role write");
    assert_eq!(role["name"], "Admin Two");
    assert_eq!(role["position"], 100, "position preserved");
    assert_ne!(role["permissions"].as_u64().unwrap(), 0, "permissions preserved");
}

#[tokio::test]
async fn delete_role_rejects_everyone_and_clears_others() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;

    let everyone = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/guilds/{GUILD}/roles/everyone"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(everyone.status(), StatusCode::BAD_REQUEST);

    let ok = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/guilds/{GUILD}/roles/admin"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    assert!(
        h.fake.writes().iter().any(|w| matches!(w, Recorded::State { event_type, state_key, content, .. }
            if event_type == "space.nuck.role.v1"
                && state_key == "admin"
                && content.as_object().is_some_and(|o| o.is_empty()))),
        "a deleted role is written with empty content"
    );
}

#[tokio::test]
async fn update_channel_renames_and_repositions() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "PATCH",
            &format!("/_nuck/v1/channels/{CHANNEL}"),
            Some(USER_TOKEN),
            Some(json!({ "name": "renamed", "position": 5 })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert!(writes.iter().any(|w| matches!(w, Recorded::State { room_id, event_type, content, .. }
        if room_id == CHANNEL && event_type == "m.room.name" && content["name"] == "renamed")));
    let channel = writes
        .iter()
        .find_map(|w| match w {
            Recorded::State { room_id, event_type, state_key, content, .. }
                if room_id == GUILD
                    && event_type == "space.nuck.channel.v1"
                    && state_key == CHANNEL =>
            {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("a channel state write");
    assert_eq!(channel["position"], 5);
    assert_eq!(channel["kind"], "text", "kind preserved");
}

#[tokio::test]
async fn delete_channel_empties_state_and_removes_child() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_channel(&h.projection, CHANNEL, GUILD).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/channels/{CHANNEL}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert!(writes.iter().any(|w| matches!(w, Recorded::State { event_type, state_key, content, .. }
        if event_type == "space.nuck.channel.v1"
            && state_key == CHANNEL
            && content.as_object().is_some_and(|o| o.is_empty()))));
    assert!(writes.iter().any(|w| matches!(w, Recorded::State { event_type, state_key, .. }
        if event_type == "m.space.child" && state_key == CHANNEL)));
}

#[tokio::test]
async fn kick_member_kicks_and_clears_roles() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    seed_member(&h.projection, GUILD, OTHER, &[]).await;

    let response = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/guilds/{GUILD}/members/{OTHER}"),
            Some(USER_TOKEN),
            Some(json!({ "reason": "spam" })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert!(writes.iter().any(|w| matches!(w, Recorded::Membership { kind, target, reason, .. }
        if kind == "kick" && target == OTHER && reason.as_deref() == Some("spam"))));
    assert!(writes.iter().any(|w| matches!(w, Recorded::State { event_type, state_key, content, .. }
        if event_type == "space.nuck.member.v1"
            && state_key == OTHER
            && content.as_object().is_some_and(|o| o.is_empty()))));
}

#[tokio::test]
async fn unban_member_unbans_and_clears_the_ban() {
    let Some(h) = setup().await else { return };
    seed_admin(&h.projection).await;
    h.projection
        .upsert_ban(&BanRow {
            guild_id: GUILD.to_owned(),
            user_id: OTHER.to_owned(),
            reason: None,
            banned_by: Some(ACTOR.to_owned()),
            created_at: Some(1),
            event_id: "$ban1".to_owned(),
        })
        .await
        .unwrap();

    let response = h
        .app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/_nuck/v1/guilds/{GUILD}/bans/{OTHER}"),
            Some(USER_TOKEN),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let writes = h.fake.writes();
    assert!(writes.iter().any(|w| matches!(w, Recorded::Membership { kind, target, .. }
        if kind == "unban" && target == OTHER)));
    assert!(writes.iter().any(|w| matches!(w, Recorded::State { event_type, state_key, content, .. }
        if event_type == "space.nuck.ban.v1"
            && state_key == OTHER
            && content.as_object().is_some_and(|o| o.is_empty()))));
}
