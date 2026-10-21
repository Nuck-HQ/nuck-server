//! nuck-server — the Nuck backend (ADR 0003).
//!
//! A Rust BFF in front of an **unmodified Tuwunel** homeserver. The Nuck client talks
//! only to this service (ADR 0001); nuck-server proxies the Matrix C-S API to Tuwunel
//! and adds the `/_nuck/v1` surface (ADR 0013), enforcing Nuck permissions and
//! projecting federated state into Postgres (ADR 0019).
//!
//! Module map:
//! - [`api`] — the HTTP surface (`/_nuck/v1`, health, and the C-S proxy).
//! - [`authorization`] — server-side authorization over projected guild state (ADR 0041).
//! - [`bot`] — the privileged action bot that performs privileged substrate writes (ADR 0041).
//! - [`config`] — environment configuration.
//! - [`db`] — the Postgres pool for the derived index (ADR 0015).
//! - [`matrix`] — the Tuwunel substrate client (ADR 0028).
//! - [`permissions`] — the permission engine and power-level mirroring (ADR 0006/0020).
//! - [`projection`] — the federated-event → Postgres pipeline (ADR 0019).
//! - [`federation`] — Nuck server-to-server endpoints (ADR 0010).
//! - [`gateway`] — the bot gateway WebSocket (ADR 0018).

pub mod api;
pub mod authorization;
pub mod bot;
pub mod config;
pub mod db;
pub mod federation;
pub mod gateway;
pub mod keys;
pub mod matrix;
pub mod permissions;
pub mod projection;
pub mod state;
