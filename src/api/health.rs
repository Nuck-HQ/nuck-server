//! Liveness/readiness probe.

/// `GET /healthz` — liveness.
pub async fn healthz() -> &'static str {
    "ok"
}
