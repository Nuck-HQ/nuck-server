//! Environment configuration.
//!
//! Secrets (the Tuwunel admin token, AppService tokens) are injected by the installer
//! into `.env` or the container environment and never committed (ADR 0028).

use std::net::SocketAddr;

use anyhow::{Context, Result};

use crate::matrix::registration::PROJECTOR_ID;

/// Default reconcile cadence, in seconds (ADR 0019).
const DEFAULT_RECONCILE_INTERVAL_SECS: u64 = 300;
/// Default bound on messages fetched per channel when backfilling search.
const DEFAULT_MESSAGE_BACKFILL_LIMIT: i64 = 200;

/// Runtime configuration for nuck-server.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address the HTTP API binds to.
    pub bind_addr: SocketAddr,
    /// This server's Matrix `server_name` (its federation identity, ADR 0009).
    pub server_name: String,
    /// Postgres connection string for the derived index (ADR 0015). `None` runs
    /// without the projection store (development only).
    pub database_url: Option<String>,
    /// Base URL of the local Tuwunel instance. Never internet-facing (ADR 0028).
    pub tuwunel_url: String,
    /// Tuwunel admin token, used only for the narrow privileged operations
    /// (ADR 0028). Optional so the server can start without it in development.
    pub tuwunel_admin_token: Option<String>,

    /// The AppService `hs_token`: the shared secret Tuwunel presents on every
    /// transaction push (ADR 0019). Optional in development.
    pub appservice_hs_token: Option<String>,
    /// The AppService `as_token`: the secret this service presents to Tuwunel
    /// (ADR 0028). Needed to act as AppService-managed users.
    pub appservice_as_token: Option<String>,
    /// The privileged **action bot** user that performs privileged writes (ADR 0041).
    /// AppService-managed, like the projector.
    pub bot_user: Option<String>,
    /// The AppService token the action bot authenticates with (ADR 0028). Defaults to
    /// `appservice_as_token` when unset.
    pub bot_token: Option<String>,
    /// The projector user id this AppService claims (ADR 0019).
    pub appservice_user: Option<String>,
    /// Registration id for the projector AppService.
    pub appservice_id: String,
    /// Base URL Tuwunel pushes transactions to, e.g.
    /// `http://nuck-server:8008/_matrix/app/v1` (ADR 0019). `None` = receive-only.
    pub appservice_url: Option<String>,
    /// Where to write the generated registration file (Tuwunel's `appservice_dir`).
    /// `None` disables registration-file management.
    pub appservice_registration_path: Option<String>,

    /// How often the reconciliation loop re-reads room state (ADR 0019).
    pub reconcile_interval_secs: u64,
    /// Whether to reconcile once on startup, before serving wall-clock passes.
    pub reconcile_on_startup: bool,
    /// Bound on messages fetched per channel during the startup backfill. `None`
    /// disables message backfill.
    pub message_backfill_limit: Option<i64>,

    /// Registration policy (ADR 0037): `open`, `token`, `approval`, or `closed`.
    pub registration_mode: String,
    /// The shared token required in `token` mode.
    pub registration_token: Option<String>,
    /// Operator admins (user ids). Server admins gate the registration queue; they are
    /// not a guild role (ADR 0037).
    pub admin_users: Vec<String>,
}

impl Config {
    /// Build the configuration from the environment, applying development defaults.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            bind_addr: env_or("NUCK_BIND_ADDR", "127.0.0.1:8008")
                .parse()
                .context("NUCK_BIND_ADDR must be a socket address")?,
            server_name: env_or("NUCK_SERVER_NAME", "localhost"),
            database_url: env_opt("DATABASE_URL"),
            tuwunel_url: env_or("TUWUNEL_URL", "http://localhost:8008"),
            tuwunel_admin_token: env_opt("TUWUNEL_ADMIN_TOKEN"),
            appservice_hs_token: env_opt("APPSERVICE_HS_TOKEN"),
            appservice_as_token: env_opt("APPSERVICE_AS_TOKEN"),
            appservice_user: env_opt("APPSERVICE_USER"),
            bot_user: env_opt("NUCK_BOT_USER"),
            bot_token: env_opt("NUCK_BOT_TOKEN"),
            appservice_id: env_or("APPSERVICE_ID", PROJECTOR_ID),
            appservice_url: env_opt("APPSERVICE_URL"),
            appservice_registration_path: env_opt("APPSERVICE_REGISTRATION_PATH"),
            reconcile_interval_secs: env_parse(
                "NUCK_RECONCILE_INTERVAL_SECS",
                DEFAULT_RECONCILE_INTERVAL_SECS,
            )?,
            reconcile_on_startup: env_bool("NUCK_RECONCILE_ON_STARTUP", true),
            message_backfill_limit: match env_opt("NUCK_MESSAGE_BACKFILL_LIMIT") {
                Some(raw) if raw.eq_ignore_ascii_case("none") => None,
                Some(raw) => Some(raw.parse().context("NUCK_MESSAGE_BACKFILL_LIMIT must be an integer or `none`")?),
                None => Some(DEFAULT_MESSAGE_BACKFILL_LIMIT),
            },
            registration_mode: env_or("NUCK_REGISTRATION_MODE", "token"),
            registration_token: env_opt("NUCK_REGISTRATION_TOKEN"),
            admin_users: env_opt("NUCK_ADMIN_USERS")
                .map(|raw| {
                    raw.split(',')
                        .map(|user| user.trim().to_owned())
                        .filter(|user| !user.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

/// An optional env var; empty means unset (so `KEY=` in a compose file is not a
/// bogus empty secret).
fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match std::env::var(key) {
        Ok(raw) => raw.parse().with_context(|| format!("{key} is invalid")),
        Err(_) => Ok(default),
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(raw) => !matches!(raw.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
        Err(_) => default,
    }
}
