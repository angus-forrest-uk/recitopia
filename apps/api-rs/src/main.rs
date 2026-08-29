use std::{error::Error, sync::Arc};

use recitopia_api_rs::{
    AppState,
    assets::AssetManager,
    config::{Config, StoreMode},
    duckdb_store::DuckStore,
    logging,
    pipeline::PipelineService,
    router,
    store::{ReadStore, WriteStore},
};
use tokio::{net::TcpListener, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    logging::init()?;
    let config = Config::from_env()?;
    let pipeline = Arc::new(PipelineService::new(config.pipeline.clone())?);
    let store = Arc::new(DuckStore::open(&config.database)?);
    let metadata = store.metadata().clone();
    tracing::info!(
        event = "store_ready",
        path = %config.database.path.display(),
        mode = ?metadata.mode,
        duckdb_version = metadata.duckdb_version,
        table_count = metadata.table_count,
        "DuckDB store is ready"
    );

    let read_store: Arc<dyn ReadStore> = store.clone();
    let state = if config.database.mode == StoreMode::ReadWrite {
        let write_store: Arc<dyn WriteStore> = store;
        AppState::with_write_store(read_store, write_store)
    } else {
        AppState::new(read_store)
    }
    .with_assets(Arc::new(AssetManager::new(config.assets.clone())))
    .with_pipeline(pipeline);
    let app = router(state);
    let listener = TcpListener::bind(config.socket_addr()).await?;
    let address = listener.local_addr()?;
    tracing::info!(
        event = "api_listening",
        implementation = "rust",
        host = %address.ip(),
        port = address.port(),
        "Recitopia Rust API listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!(event = "api_stopped", "Recitopia Rust API stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            logging::fault("shutdown_signal_failed", &error.to_string());
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => logging::fault("shutdown_signal_failed", &error.to_string()),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!(event = "shutdown_requested", "shutdown signal received");
}
