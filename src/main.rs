//! nuck-server entry point.

use std::sync::Arc;
use std::time::Duration;

use nuck_server::matrix::{SubstrateClient, registration};
use nuck_server::projection::Projection;
use nuck_server::projection::reconcile::Reconciler;
use nuck_server::state::AppState;
use nuck_server::{api, config::Config, db};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("nuck_server=info,tower_http=info")),
        )
        .init();

    let config = Arc::new(Config::from_env()?);

    // The derived index is optional so the server can run in development without
    // Postgres (ADR 0019). In production DATABASE_URL is always set by the installer.
    let projection = match &config.database_url {
        Some(url) => match db::connect(url).await {
            Ok(pool) => match db::migrate(&pool).await {
                Ok(()) => Some(Projection::new(pool)),
                Err(e) => {
                    tracing::error!("migrations failed, running without the projection store: {e}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("database unavailable, running without the projection store: {e}");
                None
            }
        },
        None => {
            tracing::warn!("DATABASE_URL not set; running without the projection store");
            None
        }
    };

    write_appservice_registration(&config);

    let substrate = SubstrateClient::from_config(&config);
    spawn_reconciliation(&config, projection.as_ref(), &substrate);

    let state = Arc::new(AppState { config: config.clone(), projection, substrate });
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;

    tracing::info!(addr = %config.bind_addr, server_name = %config.server_name, "nuck-server listening");
    axum::serve(listener, app).await?;

    Ok(())
}

/// Generate the projector's AppService registration file, when a path is configured
/// (ADR 0019/0028).
///
/// Tuwunel loads `appservice_dir` registrations on startup, so writing the file is
/// only half the job: the operator restarts Tuwunel, or applies the same YAML live
/// with `!admin appservices register`.
fn write_appservice_registration(config: &Config) {
    if config.appservice_registration_path.is_none() {
        return;
    }
    let registration = match registration::AppServiceRegistration::from_config(config) {
        Ok(registration) => registration,
        Err(e) => {
            tracing::warn!("not writing the AppService registration: {e}");
            return;
        }
    };
    let path = match registration::registration_path(config) {
        Ok(path) => path,
        Err(e) => {
            tracing::error!("invalid APPSERVICE_REGISTRATION_PATH: {e}");
            return;
        }
    };
    match registration.ensure_file(path) {
        Ok(true) => tracing::info!(
            path = %path.display(),
            "wrote the AppService registration; restart Tuwunel to load it, or run `!admin appservices register`"
        ),
        Ok(false) => tracing::debug!(path = %path.display(), "AppService registration is current"),
        Err(e) => tracing::error!("failed to write the AppService registration: {e}"),
    }
}

/// Run the startup reconciliation pass and spawn the periodic loop (ADR 0019).
///
/// Needs both the projection store and the Tuwunel admin API; without either, the
/// pipeline still works from the AppService push alone, so this only warns.
fn spawn_reconciliation(config: &Config, projection: Option<&Projection>, substrate: &SubstrateClient) {
    let Some(projection) = projection else {
        return;
    };
    if !substrate.admin_available() {
        tracing::warn!("no TUWUNEL_ADMIN_TOKEN; reconciliation is disabled");
        return;
    }

    let backfill_limit = config
        .message_backfill_limit
        .filter(|limit| *limit > 0)
        .map(|limit| limit.min(i64::from(u32::MAX)) as u32);

    let reconciler = Reconciler::new(
        projection.clone(),
        substrate.clone(),
        config.appservice_user.clone(),
        Duration::from_secs(config.reconcile_interval_secs.max(1)),
        backfill_limit,
    );

    let on_startup = config.reconcile_on_startup;
    tokio::spawn(async move {
        if on_startup {
            match reconciler.reconcile_once(true).await {
                Ok(report) => tracing::info!(
                    rooms = report.rooms,
                    ops = report.ops,
                    backfilled = report.backfilled,
                    failures = report.failures,
                    "startup reconciliation complete"
                ),
                Err(e) => tracing::warn!("startup reconciliation failed: {e}"),
            }
        }
        reconciler.run().await;
    });
}
