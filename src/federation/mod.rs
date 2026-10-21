//! Nuck federation: capability negotiation and server-to-server endpoints
//! (ADR 0010).
//!
//! Transport is **Matrix federation** (ADR 0002); Nuck adds a capability/version
//! check and the S2S endpoints listed in `docs/protocol/api.md`, such as
//! `POST /_nuck/v1/federation/invites/{code}/redeem` (ADR 0011).
//!
//! TODO: implement `GET /_nuck/v1/federation/server-info` peer checks, invite
//! redemption, and the blocklist pipeline (ADR 0024) that compiles subscribable
//! lists into Tuwunel's `forbidden_remote_server_names` config and reloads.
