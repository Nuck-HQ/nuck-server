//! The bot gateway WebSocket (ADR 0018).
//!
//! Bots connect **out** to nuck-server (no public endpoint required) and receive
//! events from the projection pipeline. Distinct from the deferred presence channel
//! (ADR 0017).
//!
//! TODO: implement authentication, subscription/filtering, fan-out, and backpressure.
