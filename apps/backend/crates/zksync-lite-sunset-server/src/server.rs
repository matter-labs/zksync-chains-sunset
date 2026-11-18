use eyre::WrapErr;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};
use zksync_lite_sunset_core::claims::worker as claim_worker;
use zksync_lite_sunset_core::world::World;

use crate::app;
use crate::config::Config;
use crate::shutdown::shutdown_signal;
use crate::state::AppState;

pub async fn run() -> eyre::Result<()> {
    init_tracing();

    let config = Config::load()?;
    let addr = config.server.addr()?;
    let metrics_addr = config.metrics.addr()?;

    let world = World::new(config.core).await?;
    let state = AppState::new(world.clone());
    let (router, ops_router) = app::app_pair_with_state_and_cors(state, config.cors);
    let listener = TcpListener::bind(addr)
        .await
        .wrap_err_with(|| format!("failed to bind to {addr}"))?;

    info!(%addr, "listening");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let worker_handle = {
        let world = world.clone();
        let shutdown_rx = shutdown_rx.clone();
        tokio::spawn(async move { claim_worker::run(world, shutdown_rx).await })
    };

    if !config.metrics.enabled {
        let signal_task = tokio::spawn(async move {
            shutdown_signal().await;
            let _ = shutdown_tx.send(true);
        });
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(wait_for_shutdown(shutdown_rx.clone()))
            .await
            .wrap_err("server error");
        let _ = worker_handle.await;
        signal_task.abort();
        return result;
    }

    let metrics_listener = TcpListener::bind(metrics_addr)
        .await
        .wrap_err_with(|| format!("failed to bind internal ops listener to {metrics_addr}"))?;

    info!(%metrics_addr, "listening for internal ops endpoints");

    let shutdown_task = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    });

    let public_server = axum::serve(listener, router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx.clone()));
    let internal_metrics_server = axum::serve(metrics_listener, ops_router)
        .with_graceful_shutdown(wait_for_shutdown(shutdown_rx.clone()));

    let result = tokio::try_join!(public_server, internal_metrics_server)
        .map(|_| ())
        .wrap_err("server error");

    let _ = worker_handle.await;
    shutdown_task.abort();
    result
}

async fn wait_for_shutdown(mut shutdown_rx: watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }

        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
