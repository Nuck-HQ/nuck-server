//! Shared Nuck protocol types — the single source of truth for the client/server
//! contract (ADR 0027), carrying its own version axis (ADR 0026).
//!
//! Contains:
//! - [`events`] — the custom `space.nuck.*` event registry (ADR 0014).
//! - [`permissions`] — the permission bitfield and power-level mirroring (ADR 0020).
//! - [`authorization`] — the role/override resolution engine (ADR 0020/0041).
//! - [`api`] — `/_nuck/v1` request/response DTOs (ADR 0013).
//!
//! Every type here is exchanged over the wire between Nuck servers, or between
//! nuck-server and the Nuck client. Breaking changes bump the *protocol* version
//! (ADR 0026); additive changes keep the type name and rely on tolerant parsing
//! (ADR 0014).

pub mod api;
pub mod authorization;
pub mod events;
pub mod permissions;

/// The reverse-DNS event namespace shared by **every** Nuck server (ADR 0014).
///
/// This is a global protocol namespace, not per-server: a remote Nuck server must
/// recognize the same event types. Server identity is a separate axis (Matrix
/// `server_name` + signing keys).
pub const EVENT_NAMESPACE: &str = "space.nuck";

/// The current Nuck protocol version, negotiated via `/_nuck/v1/server-info`
/// (ADR 0010/0026). Distinct from the product version.
pub const PROTOCOL_VERSION: u32 = 1;

/// The oldest protocol version this build still speaks. A peer below this is
/// feature-gated rather than broken (ADR 0026).
pub const PROTOCOL_MIN_VERSION: u32 = 1;
