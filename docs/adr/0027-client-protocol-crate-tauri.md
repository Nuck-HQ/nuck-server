# ADR 0027 — Client relationship: shared `nuck-protocol` crate; Tauri client

- Status: Accepted
- Date: 2026-10-21

## Context

The Nuck client is a separate application from nuck-server but must agree with it
exactly on the wire format: dozens of `space.nuck.*` events, the `/_nuck/v1` API
DTOs, the versioned permission bitfield, and the power-level mapping. It also needs
Matrix sync and E2EE (ADR 0008), for which `matrix-rust-sdk` is the only mature
implementation.

Client options considered: Wails v3 + React, Tauri + React.

## Decision

- **The client is Tauri (Rust core) + React UI.**
- **A published, independently versioned `nuck-protocol` crate is the single source
  of truth** for `space.nuck.*` event types (ruma-based), `/_nuck/v1` DTOs, the
  permission bitfield, and the power-level mapping. Server and client both depend on
  it, so the contract is **compile-time verified**.
- The client's Rust core also uses **`matrix-rust-sdk`** for sync and E2EE.

**Wails is rejected:** its backend is **Go**, so it can use neither `nuck-protocol`
nor `matrix-rust-sdk` — it would require reimplementing the protocol types and the
entire Matrix client stack in Go.

## Consequences

- Client/server contract drift is impossible for anything expressed in the crate.
- `nuck-protocol` has its **own version axis** (ADR 0026) and is released
  independently of both server and client.
- The client and server remain **separate repositories**; the crate is the shared
  dependency.
- UI is React over a Rust core — web-tech velocity with native capability
  (local storage, notifications, OS integration).
- If a non-Rust client is ever needed, it consumes the protocol via FFI/WASM or
  generated bindings — the crate stays the source of truth.
