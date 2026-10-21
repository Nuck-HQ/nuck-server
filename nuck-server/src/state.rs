//! State shared by every handler.

use std::sync::Arc;

use crate::config::Config;
use crate::matrix::SubstrateClient;
use crate::projection::Projection;

/// Application state.
#[derive(Clone)]
pub struct AppState {
    /// Runtime configuration.
    pub config: Arc<Config>,
    /// The derived index (ADR 0019). `None` when no database is configured, so the
    /// server can run in development without Postgres.
    pub projection: Option<Projection>,
    /// The Tuwunel substrate client (ADR 0028): token validation and the action bot's
    /// privileged writes (ADR 0041).
    pub substrate: SubstrateClient,
}
