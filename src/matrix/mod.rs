//! Client for the Tuwunel substrate (ADR 0003/0028).
//!
//! nuck-server talks to the local Tuwunel over the Matrix C-S API and the
//! Synapse-compatible admin API at `/_synapse/admin` (ADR 0028). The substrate is
//! **never** internet-facing: the client only reaches nuck-server.
//!
//! This module owns the **substrate boundary types** ([`RawEvent`]) and the calls the
//! rest of the server makes:
//!
//! - Admin reads for reconciliation (ADR 0019) — room discovery, state, messages, and
//!   joining a user (`GET/POST /_synapse/admin/v1/...`).
//! - Caller identity — `GET /_matrix/client/v3/account/whoami`.
//! - The **action bot**'s writes (ADR 0041) — privileged state events and membership
//!   changes, made by an AppService user.

pub mod registration;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, Method, StatusCode};
use serde::Deserialize;

use crate::config::Config;

/// A raw Matrix event as delivered by the AppService push or the admin API.
///
/// Deliberately loose: the projection maps it tolerantly (ADR 0014). This is the
/// input shape of the whole pipeline (ADR 0019).
#[derive(Debug, Clone, Deserialize)]
pub struct RawEvent {
    pub event_id: String,
    pub room_id: String,
    pub sender: String,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub state_key: Option<String>,
    #[serde(default)]
    pub origin_server_ts: Option<u64>,
    #[serde(default)]
    pub content: serde_json::Value,
}

impl RawEvent {
    /// The state key, or `""` for singleton state and message events.
    #[must_use]
    pub fn state_key(&self) -> &str {
        self.state_key.as_deref().unwrap_or("")
    }
}

/// A page of a room's timeline (`GET /rooms/{id}/messages`).
#[derive(Debug, Deserialize)]
struct MessagesPage {
    #[serde(default)]
    chunk: Vec<RawEvent>,
}

/// The rooms a user has joined (`GET /joined_rooms`).
#[derive(Debug, Deserialize)]
struct JoinedRooms {
    #[serde(default)]
    joined_rooms: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WhoAmI {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct SentEvent {
    event_id: String,
}

#[derive(Debug, Deserialize)]
struct CreatedRoom {
    room_id: String,
}

/// A user's profile (the parts Nuck surfaces).
#[derive(Debug, Deserialize)]
struct ProfileBody {
    #[serde(default)]
    displayname: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

/// A successful C-S registration (ADR 0037 `open`/`token` modes).
#[derive(Debug, Deserialize)]
struct RegisterSuccess {
    user_id: String,
    access_token: String,
    #[serde(default)]
    device_id: String,
}

/// The 401 body that starts the `m.login.dummy` registration flow.
#[derive(Debug, Deserialize)]
struct RegisterChallenge {
    #[serde(default)]
    session: String,
}

/// The outcome of one C-S registration step.
enum RegisterStep {
    Done(RegisterSuccess),
    Challenge(String),
}

/// A state event as the C-S API returns it (it may omit `room_id`).
#[derive(Debug, Deserialize)]
struct ClientStateEvent {
    event_id: String,
    #[serde(default)]
    room_id: Option<String>,
    sender: String,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    state_key: Option<String>,
    #[serde(default)]
    origin_server_ts: Option<u64>,
    #[serde(default)]
    content: serde_json::Value,
}

/// A handle to the local Tuwunel instance.
#[derive(Clone)]
pub struct SubstrateClient {
    http: Client,
    base_url: String,
}

impl SubstrateClient {
    /// Build a substrate client from configuration.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        // Bounded timeouts: neither reconciliation nor a request path may block forever
        // on a wedged substrate.
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { http, base_url: config.tuwunel_url.trim_end_matches('/').to_owned() }
    }

    /// The base URL of the substrate (for diagnostics).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The underlying HTTP client, for the C-S proxy.
    #[must_use]
    pub fn http(&self) -> &Client {
        &self.http
    }

    // --- Identity (ADR 0009/0028) ------------------------------------------

    /// Resolve a client access token to its user id via the substrate.
    ///
    /// `Ok(None)` means the substrate **rejected** the token (an invalid or expired
    /// session); `Err` means the substrate was unreachable or misbehaved. The caller
    /// must distinguish the two (ADR 0026: never silently misbehave).
    pub async fn whoami(&self, token: &str) -> Result<Option<String>> {
        let url = format!("{}/_matrix/client/v3/account/whoami", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        match response.status() {
            status if status.is_success() => {
                let body: WhoAmI = response
                    .json()
                    .await
                    .context("whoami returned an unexpected body")?;
                Ok(Some(body.user_id))
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Ok(None),
            status => bail!("whoami returned {status}"),
        }
    }

    // --- Identity reads (ADR 0009/0028) ------------------------------------

    /// A user's profile (display name / avatar), or `None` when the substrate has none.
    pub async fn profile(
        &self,
        token: &str,
        user_id: &str,
    ) -> Result<Option<(Option<String>, Option<String>)>> {
        let path = format!("/_matrix/client/v3/profile/{}", encode_path(user_id));
        let body: Option<ProfileBody> = self.call_optional(Method::GET, &path, token).await?;
        Ok(body.map(|p| (p.displayname, p.avatar_url)))
    }

    /// A user's account-data content (ADR 0030), or `None` when unset.
    pub async fn account_data(
        &self,
        token: &str,
        user_id: &str,
        event_type: &str,
    ) -> Result<Option<serde_json::Value>> {
        let path = format!(
            "/_matrix/client/v3/user/{}/account_data/{}",
            encode_path(user_id),
            encode_path(event_type),
        );
        self.call_optional(Method::GET, &path, token).await
    }

    // --- Projector reads (ADR 0019) ----------------------------------------

    /// A page of a room's timeline, read **as a member** (the projector), newest first.
    ///
    /// Reconciliation backfill (ADR 0029): the projector is joined to every guild room,
    /// so it reads the room's history without any admin privilege.
    pub async fn room_messages_as(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
        limit: u32,
    ) -> Result<Vec<RawEvent>> {
        let mut path = format!(
            "/_matrix/client/v3/rooms/{}/messages?dir=b&limit={}",
            encode_path(room_id),
            limit
        );
        if let Some(user_id) = user_id {
            path.push_str("&user_id=");
            path.push_str(&encode_query(user_id));
        }
        let page: MessagesPage = self.call(Method::GET, &path, token, None).await?;
        Ok(page.chunk)
    }

    /// The rooms a user has joined, read **as that user** (projector discovery).
    ///
    /// The projector is a member of every guild room (ADR 0019), so its joined rooms
    /// are exactly the rooms worth reconciling — no admin room listing required.
    pub async fn joined_rooms(&self, token: &str, user_id: Option<&str>) -> Result<Vec<String>> {
        let mut path = "/_matrix/client/v3/joined_rooms".to_owned();
        append_user_id(&mut path, user_id);
        let body: JoinedRooms = self.call(Method::GET, &path, token, None).await?;
        Ok(body.joined_rooms)
    }

    /// Register a user through the substrate's C-S API (ADR 0037 `open`/`token` modes).
    ///
    /// nuck-server applies the registration *policy*; the substrate still performs the
    /// account creation. The `m.login.dummy` two-step flow is completed here. Returns
    /// `(user_id, access_token, device_id)`.
    pub async fn register(&self, username: &str, password: &str) -> Result<(String, String, String)> {
        let path = "/_matrix/client/v3/register";
        let first = serde_json::json!({ "username": username, "password": password });
        let session = match self.register_step(path, first).await? {
            RegisterStep::Done(success) => {
                return Ok((success.user_id, success.access_token, success.device_id));
            }
            RegisterStep::Challenge(session) => session,
        };
        let body = serde_json::json!({
            "username": username,
            "password": password,
            "auth": { "type": "m.login.dummy", "session": session },
        });
        match self.register_step(path, body).await? {
            RegisterStep::Done(success) => Ok((success.user_id, success.access_token, success.device_id)),
            RegisterStep::Challenge(_) => bail!("registration requires interactive authentication"),
        }
    }

    /// One registration step: `Done` on success, `Challenge` when a session is required.
    async fn register_step(&self, path: &str, body: serde_json::Value) -> Result<RegisterStep> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;
        match response.status() {
            status if status.is_success() => {
                let success: RegisterSuccess =
                    response.json().await.context("register returned an unexpected body")?;
                Ok(RegisterStep::Done(success))
            }
            StatusCode::UNAUTHORIZED => {
                let challenge: RegisterChallenge = response
                    .json()
                    .await
                    .context("register challenge returned an unexpected body")?;
                Ok(RegisterStep::Challenge(challenge.session))
            }
            status => {
                let detail = response.text().await.unwrap_or_default();
                bail!("register returned {status}: {detail}")
            }
        }
    }

    // --- Action-bot writes (ADR 0041) --------------------------------------

    /// Send a state event **as** an AppService user, returning the new event id.
    ///
    /// This is how the action bot performs a privileged write (ADR 0041): the client
    /// authenticates with the bot's AppService token and names the bot via `user_id`.
    pub async fn send_state_event(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
        event_type: &str,
        state_key: &str,
        content: serde_json::Value,
    ) -> Result<String> {
        // A singleton state event has no trailing state-key segment.
        let mut path = format!(
            "/_matrix/client/v3/rooms/{}/state/{}",
            encode_path(room_id),
            encode_path(event_type),
        );
        if !state_key.is_empty() {
            path.push('/');
            path.push_str(&encode_path(state_key));
        }
        append_user_id(&mut path, user_id);
        let sent: SentEvent = self.call(Method::PUT, &path, token, Some(content)).await?;
        Ok(sent.event_id)
    }

    // --- Client-Server writes (M2/M3) --------------------------------------

    /// A room's full current state, read as a **member** over the C-S API (no admin
    /// token needed). Used to backfill state written before the projector joined.
    pub async fn room_state_as(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
    ) -> Result<Vec<RawEvent>> {
        let mut path = format!("/_matrix/client/v3/rooms/{}/state", encode_path(room_id));
        append_user_id(&mut path, user_id);
        let events: Vec<ClientStateEvent> = self.call(Method::GET, &path, token, None).await?;
        Ok(events
            .into_iter()
            .map(|e| RawEvent {
                event_id: e.event_id,
                room_id: e.room_id.unwrap_or_else(|| room_id.to_owned()),
                sender: e.sender,
                event_type: e.event_type,
                state_key: e.state_key,
                origin_server_ts: e.origin_server_ts,
                content: e.content,
            })
            .collect())
    }

    /// Create a room as `user_id` (AppService users pass `Some`, a real user passes
    /// `None` and their own token). Returns the new room id.
    pub async fn create_room(
        &self,
        token: &str,
        user_id: Option<&str>,
        body: serde_json::Value,
    ) -> Result<String> {
        let mut path = "/_matrix/client/v3/createRoom".to_owned();
        append_user_id(&mut path, user_id);
        let created: CreatedRoom = self.call(Method::POST, &path, token, Some(body)).await?;
        Ok(created.room_id)
    }

    /// Join a room as `user_id`.
    pub async fn join_room(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
    ) -> Result<()> {
        let mut path = format!("/_matrix/client/v3/rooms/{}/join", encode_path(room_id));
        append_user_id(&mut path, user_id);
        let _: serde_json::Value = self.call(Method::POST, &path, token, None).await?;
        Ok(())
    }

    /// Invite `target` to a room as `user_id`.
    pub async fn invite(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
        target: &str,
    ) -> Result<()> {
        let mut path = format!("/_matrix/client/v3/rooms/{}/invite", encode_path(room_id));
        append_user_id(&mut path, user_id);
        let _: serde_json::Value = self
            .call(Method::POST, &path, token, Some(serde_json::json!({ "user_id": target })))
            .await?;
        Ok(())
    }

    /// Send a to-device event **as** the token's user (ADR 0030 friend requests).
    pub async fn send_to_device(
        &self,
        token: &str,
        event_type: &str,
        txn_id: &str,
        messages: serde_json::Value,
    ) -> Result<()> {
        let path = format!(
            "/_matrix/client/v3/sendToDevice/{}/{}",
            encode_path(event_type),
            encode_path(txn_id),
        );
        let body = serde_json::json!({ "messages": messages });
        let _: serde_json::Value = self.call(Method::PUT, &path, token, Some(body)).await?;
        Ok(())
    }

    /// Change a room membership **as** an AppService user: `ban`, `kick`, or `unban`.
    pub async fn set_membership(
        &self,
        token: &str,
        user_id: Option<&str>,
        room_id: &str,
        action: &str,
        target: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        let mut path = format!(
            "/_matrix/client/v3/rooms/{}/{}",
            encode_path(room_id),
            encode_path(action),
        );
        append_user_id(&mut path, user_id);
        let mut body = serde_json::json!({ "user_id": target });
        if let Some(reason) = reason {
            body["reason"] = serde_json::Value::String(reason.to_owned());
        }
        let _: serde_json::Value = self.call(Method::POST, &path, token, Some(body)).await?;
        Ok(())
    }

    // --- Plumbing ----------------------------------------------------------

    /// Like [`Self::call`], but maps a 404 to `Ok(None)` instead of an error.
    async fn call_optional<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        token: &str,
    ) -> Result<Option<T>> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .http
            .request(method.clone(), &url)
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("{method} {url} failed"))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            bail!("{method} {path} returned {status}: {detail}");
        }
        Ok(Some(
            response
                .json::<T>()
                .await
                .with_context(|| format!("{method} {path} returned an unexpected body"))?,
        ))
    }

    /// Perform a request authenticated with `token`, decoding a JSON body.
    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let mut request = self.http.request(method.clone(), &url).bearer_auth(token);
        if let Some(value) = body {
            request = request.json(&value);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("{method} {url} failed"))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            bail!("{method} {path} not found (is this a Tuwunel admin API?)");
        }
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            bail!("{method} {path} returned {status}: {detail}");
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("{method} {path} returned an unexpected body"))
    }
}

impl std::fmt::Debug for SubstrateClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubstrateClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// Append the AppService `user_id` query parameter when masquerading. A real user's
/// own token needs no parameter.
fn append_user_id(path: &mut String, user_id: Option<&str>) {
    if let Some(user_id) = user_id {
        path.push_str("?user_id=");
        path.push_str(&encode_query(user_id));
    }
}

/// Percent-encode a value for use as a single URL path segment.
///
/// Matrix ids contain `!`/`:`/`@`; encoding them is what Synapse's own clients do and
/// avoids any ambiguity about which characters are safe in a path.
fn encode_path(value: &str) -> String {
    encode(value, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
    })
}

/// Percent-encode a query-string value (`application/x-www-form-urlencoded` rules).
fn encode_query(value: &str) -> String {
    encode(value, |b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

fn encode(value: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if keep(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_encoding_escapes_matrix_ids() {
        assert_eq!(encode_path("!room:example.org"), "%21room%3Aexample.org");
        assert_eq!(encode_path("@u:example.org"), "%40u%3Aexample.org");
        // Dots are safe in a path segment, so event types pass through.
        assert_eq!(encode_path("space.nuck.role.v1"), "space.nuck.role.v1");
    }

    #[test]
    fn raw_event_state_key_defaults_to_empty() {
        let event: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$e",
            "room_id": "!r:example.org",
            "sender": "@a:example.org",
            "type": "space.nuck.guild.v1",
            "content": {}
        }))
        .unwrap();
        assert_eq!(event.state_key(), "");
    }

    #[test]
    fn joined_rooms_tolerates_a_missing_list() {
        let body: JoinedRooms = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(body.joined_rooms.is_empty());
    }
}
