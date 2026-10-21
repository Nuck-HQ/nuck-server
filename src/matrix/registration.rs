//! AppService registration for the projector (ADR 0019/0028).
//!
//! Tuwunel accepts registrations three ways (verified against
//! `docs/appservices.md`): an **admin-room command**, **inline server config**, and an
//! **`appservice_dir` of YAML files**. There is no HTTP registration endpoint.
//!
//! nuck-server therefore *generates the registration document* — id, URL, tokens, and
//! the projector's exclusive user namespace — and, when a directory is configured,
//! writes it as a registration file. Writing into `appservice_dir` is picked up on the
//! next Tuwunel restart; the admin-room `!admin appservices register` form of the same
//! YAML is applied live. This module is the single source of truth for that document.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::Config;

/// The registration ID used for the projector AppService.
pub const PROJECTOR_ID: &str = "nuck-projector";

/// An AppService registration document (Synapse/YAML shape, Tuwunel-compatible).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppServiceRegistration {
    pub id: String,
    /// Base URL Tuwunel pushes transactions to, or `null` for receive-only.
    pub url: Option<String>,
    pub as_token: String,
    pub hs_token: String,
    pub sender_localpart: String,
    pub namespaces: Namespaces,
    pub receive_ephemeral: bool,
}

/// The user/alias/room namespaces an AppService claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Namespaces {
    pub users: Vec<Namespace>,
    pub aliases: Vec<Namespace>,
    pub rooms: Vec<Namespace>,
}

/// One namespace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Namespace {
    pub exclusive: bool,
    pub regex: String,
}

impl AppServiceRegistration {
    /// Build the registration from configuration.
    ///
    /// Claims the projector user and, when configured, the privileged **action bot**
    /// (ADR 0041) — both are AppService-managed users. Requires the AppService tokens
    /// and the projector user; the push URL is optional (omitted = receive-only).
    /// Fails loudly rather than registering an incomplete service (ADR 0028).
    pub fn from_config(config: &Config) -> Result<Self> {
        let user = config
            .appservice_user
            .as_deref()
            .context("APPSERVICE_USER is required to build the registration")?;
        let as_token = config
            .appservice_as_token
            .as_deref()
            .context("APPSERVICE_AS_TOKEN is required to build the registration")?;
        let hs_token = config
            .appservice_hs_token
            .as_deref()
            .context("APPSERVICE_HS_TOKEN is required to build the registration")?;

        let sender_localpart = localpart(user)
            .with_context(|| format!("APPSERVICE_USER {user:?} is not a Matrix user id"))?;

        // Exclusive: no normal user may register these ids, and the AppService creates
        // them. The action bot rides the same registration (ADR 0019).
        let mut users = vec![Namespace {
            exclusive: true,
            regex: format!("^{}$", regex_escape(user)),
        }];
        if let Some(bot) = config.bot_user.as_deref() {
            localpart(bot)
                .with_context(|| format!("NUCK_BOT_USER {bot:?} is not a Matrix user id"))?;
            users.push(Namespace {
                exclusive: true,
                regex: format!("^{}$", regex_escape(bot)),
            });
        }

        Ok(Self {
            id: config.appservice_id.clone(),
            url: config.appservice_url.clone(),
            as_token: as_token.to_owned(),
            hs_token: hs_token.to_owned(),
            sender_localpart: sender_localpart.to_owned(),
            namespaces: Namespaces { users, aliases: Vec::new(), rooms: Vec::new() },
            receive_ephemeral: false,
        })
    }

    /// Render the registration as YAML (the format Tuwunel's `appservice_dir` and the
    /// admin-room command consume).
    ///
    /// Scalars are emitted as JSON strings, which are valid YAML double-quoted scalars,
    /// so escaping (regex backslashes included) is exact without a YAML dependency.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("id: {}\n", scalar(&self.id)));
        out.push_str(&format!("url: {}\n", optional_scalar(self.url.as_deref())));
        out.push_str(&format!("as_token: {}\n", scalar(&self.as_token)));
        out.push_str(&format!("hs_token: {}\n", scalar(&self.hs_token)));
        out.push_str(&format!("sender_localpart: {}\n", scalar(&self.sender_localpart)));
        out.push_str(&format!("receive_ephemeral: {}\n", self.receive_ephemeral));
        out.push_str("namespaces:\n");
        out.push_str(&namespace_list("users", &self.namespaces.users, true));
        out.push_str(&namespace_list("aliases", &self.namespaces.aliases, false));
        out.push_str(&namespace_list("rooms", &self.namespaces.rooms, false));
        out
    }

    /// Write the registration into `path` if its contents differ; returns whether the
    /// file was (re)written. Idempotent, so it is safe on every startup.
    pub fn ensure_file(&self, path: &Path) -> Result<bool> {
        let rendered = self.to_yaml();
        if std::fs::read_to_string(path).is_ok_and(|existing| existing == rendered) {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        // Write-then-rename so a reader never sees a half-written registration.
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, &rendered)
            .with_context(|| format!("writing {}", temp.display()))?;
        std::fs::rename(&temp, path)
            .with_context(|| format!("moving {} into place", temp.display()))?;
        Ok(true)
    }
}

/// YAML for one namespace list. `force_exclusive` is used for the users namespace.
fn namespace_list(name: &str, entries: &[Namespace], force_exclusive: bool) -> String {
    if entries.is_empty() {
        return format!("  {name}: []\n");
    }
    let mut out = format!("  {name}:\n");
    for entry in entries {
        let exclusive = entry.exclusive || force_exclusive;
        out.push_str(&format!("    - exclusive: {exclusive}\n"));
        out.push_str(&format!("      regex: {}\n", scalar(&entry.regex)));
    }
    out
}

/// A JSON string is a valid YAML double-quoted scalar.
fn scalar(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("{value:?}"))
}

fn optional_scalar(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), scalar)
}

/// The localpart of a Matrix user id (`@localpart:server`).
fn localpart(user_id: &str) -> Option<&str> {
    let rest = user_id.strip_prefix('@')?;
    let (local, server) = rest.split_once(':')?;
    if local.is_empty() || server.is_empty() { None } else { Some(local) }
}

/// Escape regex metacharacters so a literal id matches only itself.
fn regex_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(
            ch,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The path a registration would be written to, or an error when none is configured.
pub fn registration_path(config: &Config) -> Result<&Path> {
    let path = config
        .appservice_registration_path
        .as_deref()
        .context("APPSERVICE_REGISTRATION_PATH is not set")?;
    let path = Path::new(path);
    if path.is_dir() {
        bail!("APPSERVICE_REGISTRATION_PATH must be a file, not a directory");
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn config() -> Config {
        Config {
            bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            server_name: "example.com".to_owned(),
            database_url: None,
            tuwunel_url: "http://localhost:8008".to_owned(),
            tuwunel_admin_token: None,
            appservice_hs_token: Some("hs-secret".to_owned()),
            appservice_user: Some("@nuck-projector:example.com".to_owned()),
            appservice_as_token: Some("as-secret".to_owned()),
            bot_user: Some("@nuck-bot:example.com".to_owned()),
            bot_token: None,
            appservice_url: Some("http://nuck-server:8008/_matrix/app/v1".to_owned()),
            appservice_registration_path: None,
            appservice_id: PROJECTOR_ID.to_owned(),
            reconcile_interval_secs: 300,
            reconcile_on_startup: true,
            message_backfill_limit: Some(200),
            registration_mode: "token".to_owned(),
            registration_token: None,
            admin_users: Vec::new(),
        }
    }

    #[test]
    fn registration_claims_the_projector_and_the_action_bot() {
        let registration = AppServiceRegistration::from_config(&config()).unwrap();
        assert_eq!(registration.id, "nuck-projector");
        assert_eq!(registration.sender_localpart, "nuck-projector");
        assert_eq!(registration.namespaces.users.len(), 2);
        assert!(registration.namespaces.users.iter().all(|ns| ns.exclusive));
        assert_eq!(registration.namespaces.users[0].regex, "^@nuck-projector:example\\.com$");
        assert_eq!(registration.namespaces.users[1].regex, "^@nuck-bot:example\\.com$");
        assert_eq!(registration.url.as_deref(), Some("http://nuck-server:8008/_matrix/app/v1"));
    }

    #[test]
    fn registration_without_a_bot_claims_only_the_projector() {
        let mut cfg = config();
        cfg.bot_user = None;
        let registration = AppServiceRegistration::from_config(&cfg).unwrap();
        assert_eq!(registration.namespaces.users.len(), 1);
    }

    #[test]
    fn yaml_is_valid_and_escapes_the_regex() {
        let yaml = AppServiceRegistration::from_config(&config()).unwrap().to_yaml();
        assert!(yaml.contains("id: \"nuck-projector\""));
        assert!(yaml.contains("regex: \"^@nuck-projector:example\\\\.com$\""));
        assert!(yaml.contains("regex: \"^@nuck-bot:example\\\\.com$\""));
        assert!(yaml.contains("  aliases: []"));
        assert!(yaml.contains("  rooms: []"));
    }

    #[test]
    fn registration_requires_tokens() {
        let mut cfg = config();
        cfg.appservice_as_token = None;
        assert!(AppServiceRegistration::from_config(&cfg).is_err());

        let mut cfg = config();
        cfg.appservice_user = None;
        assert!(AppServiceRegistration::from_config(&cfg).is_err());
    }

    #[test]
    fn ensure_file_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("nuck-reg-{}", std::process::id()));
        let path = dir.join("nuck-projector.yaml");
        let registration = AppServiceRegistration::from_config(&config()).unwrap();

        assert!(registration.ensure_file(&path).unwrap(), "first write");
        assert!(!registration.ensure_file(&path).unwrap(), "unchanged is a no-op");
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("hs_token"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn localpart_parsing_rejects_non_user_ids() {
        assert_eq!(localpart("@a:b"), Some("a"));
        assert_eq!(localpart("nope"), None);
        assert_eq!(localpart("@:b"), None);
    }
}
