//! The Postgres pool for the derived index (ADR 0015).
//!
//! The store is a **rebuildable cache** over federated state (ADR 0003): if it is
//! lost, it is reconstructed by replaying events (ADR 0019).

use anyhow::Result;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Connect to Postgres and return a pool.
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run the embedded migrations (`migrations/`) against the pool.
///
/// The schema is a rebuildable derived index (ADR 0003/0015/0019), so migrations are
/// forward-only and the store may be wiped and rebuilt at any time.
pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
