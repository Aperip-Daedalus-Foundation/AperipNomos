use std::{
    error::Error,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use aperip_nomos::{
    config::ServiceConfig,
    http::{admin_router, public_router},
    server::serve_until,
    store::{StoreConfig, spawn_store},
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const STORE_QUEUE_CAPACITY: usize = 128;
const STORE_PING_INTERVAL: Duration = Duration::from_secs(2);
const STORE_PING_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONSECUTIVE_PING_FAILURES: u8 = 3;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    init_tracing();
    let config = ServiceConfig::load()?;
    let public_listener = bind_listener("public", config.public_bind_addr()).await?;
    let admin_listener = bind_listener("administrator", config.admin_bind_addr()).await?;
    let (store, store_task) = spawn_store(StoreConfig {
        database_path: config.database_path().to_path_buf(),
        page_key: config.page_key(),
        queue_capacity: STORE_QUEUE_CAPACITY,
    })?;
    let public = public_router(store.clone());
    let admin = admin_router(store.clone(), config.admin_token().to_string());
    let actor_failed = Arc::new(AtomicBool::new(false));

    tracing::info!(
        public = %config.public_bind_addr(),
        admin = %config.admin_bind_addr(),
        "AperipNomos listeners are ready"
    );
    let result = serve_until(
        public_listener,
        public,
        admin_listener,
        admin,
        shutdown_signal(store, Arc::clone(&actor_failed)),
    )
    .await;
    store_task.shutdown().await;
    result?;
    if actor_failed.load(Ordering::Acquire) {
        return Err(std::io::Error::other("RNMDB actor stopped unexpectedly").into());
    }
    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("aperip_nomos=info,tower_http=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn bind_listener(name: &str, address: SocketAddr) -> std::io::Result<TcpListener> {
    TcpListener::bind(address).await.map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("failed to bind {name} listener at {address}: {error}"),
        )
    })
}

async fn shutdown_signal(store: aperip_nomos::store::LicenseStore, actor_failed: Arc<AtomicBool>) {
    let unavailable_store = store.clone();
    tokio::select! {
        () = operating_system_shutdown() => {}
        () = unavailable_store.wait_until_unavailable() => {
            actor_failed.store(true, Ordering::Release);
            tracing::error!("RNMDB actor stopped unexpectedly");
        }
        () = store_watchdog(store) => {
            actor_failed.store(true, Ordering::Release);
            tracing::error!("RNMDB actor watchdog failed");
        }
    }
}

async fn store_watchdog(store: aperip_nomos::store::LicenseStore) {
    let mut failures = 0_u8;
    loop {
        tokio::time::sleep(STORE_PING_INTERVAL).await;
        let healthy = matches!(
            tokio::time::timeout(STORE_PING_TIMEOUT, store.ping()).await,
            Ok(Ok(()))
        );
        if record_ping_result(&mut failures, healthy) {
            return;
        }
    }
}

fn record_ping_result(failures: &mut u8, healthy: bool) -> bool {
    if healthy {
        *failures = 0;
        return false;
    }
    *failures = failures.saturating_add(1);
    *failures >= MAX_CONSECUTIVE_PING_FAILURES
}

async fn operating_system_shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if let Err(error) = result {
                        tracing::error!(%error, "failed to listen for Ctrl+C");
                    }
                }
                _ = terminate.recv() => {}
            },
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM handler");
                if let Err(error) = tokio::signal::ctrl_c().await {
                    tracing::error!(%error, "failed to listen for Ctrl+C");
                }
            }
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to listen for Ctrl+C");
    }
}

#[cfg(test)]
mod tests {
    use super::record_ping_result;

    #[test]
    fn watchdog_requires_three_consecutive_failures() {
        let mut failures = 0;
        assert!(!record_ping_result(&mut failures, false));
        assert!(!record_ping_result(&mut failures, false));
        assert_eq!(failures, 2);
        assert!(!record_ping_result(&mut failures, true));
        assert_eq!(failures, 0);
        assert!(!record_ping_result(&mut failures, false));
        assert!(!record_ping_result(&mut failures, false));
        assert!(record_ping_result(&mut failures, false));
    }
}
