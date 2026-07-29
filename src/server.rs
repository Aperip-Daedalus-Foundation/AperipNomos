use std::{
    future::{Future, IntoFuture},
    time::Duration,
};

use axum::Router;
use tokio::{net::TcpListener, sync::watch};

const SERVER_DRAIN_TIMEOUT: Duration = Duration::from_secs(25);

pub async fn serve_until<F>(
    public_listener: TcpListener,
    public_router: Router,
    admin_listener: TcpListener,
    admin_router: Router,
    shutdown: F,
) -> std::io::Result<()>
where
    F: Future<Output = ()>,
{
    let (shutdown_sender, shutdown_receiver) = watch::channel(false);
    let public_shutdown = wait_for_shutdown(shutdown_receiver.clone());
    let admin_shutdown = wait_for_shutdown(shutdown_receiver);
    let public_server = axum::serve(public_listener, public_router)
        .with_graceful_shutdown(public_shutdown)
        .into_future();
    let admin_server = axum::serve(admin_listener, admin_router)
        .with_graceful_shutdown(admin_shutdown)
        .into_future();
    coordinate_servers(
        public_server,
        admin_server,
        shutdown,
        shutdown_sender,
        SERVER_DRAIN_TIMEOUT,
    )
    .await
}

async fn coordinate_servers<P, A, S>(
    public_server: P,
    admin_server: A,
    shutdown: S,
    shutdown_sender: watch::Sender<bool>,
    drain_timeout: Duration,
) -> std::io::Result<()>
where
    P: Future<Output = std::io::Result<()>>,
    A: Future<Output = std::io::Result<()>>,
    S: Future<Output = ()>,
{
    tokio::pin!(public_server, admin_server, shutdown);

    tokio::select! {
        result = &mut public_server => {
            signal_shutdown(&shutdown_sender);
            combine_results(result, drain_peer(admin_server, drain_timeout).await)
        }
        result = &mut admin_server => {
            signal_shutdown(&shutdown_sender);
            combine_results(result, drain_peer(public_server, drain_timeout).await)
        }
        () = &mut shutdown => {
            signal_shutdown(&shutdown_sender);
            match tokio::time::timeout(drain_timeout, async {
                tokio::join!(public_server, admin_server)
            }).await {
                Ok((public_result, admin_result)) => combine_results(public_result, admin_result),
                Err(_) => Err(drain_timeout_error()),
            }
        }
    }
}

async fn drain_peer<F>(peer: F, timeout: Duration) -> std::io::Result<()>
where
    F: Future<Output = std::io::Result<()>>,
{
    tokio::time::timeout(timeout, peer)
        .await
        .map_err(|_| drain_timeout_error())?
}

fn drain_timeout_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "HTTP listener graceful shutdown timed out",
    )
}

fn combine_results(primary: std::io::Result<()>, peer: std::io::Result<()>) -> std::io::Result<()> {
    match primary {
        Err(error) => Err(error),
        Ok(()) => peer,
    }
}

fn signal_shutdown(sender: &watch::Sender<bool>) {
    let _ = sender.send(true);
}

async fn wait_for_shutdown(mut receiver: watch::Receiver<bool>) {
    if *receiver.borrow() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future, io,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use tokio::sync::watch;

    use super::coordinate_servers;

    #[tokio::test]
    async fn server_error_still_drains_signaled_peer() {
        let (shutdown_sender, mut shutdown_receiver) = watch::channel(false);
        let peer_drained = Arc::new(AtomicBool::new(false));
        let peer_flag = Arc::clone(&peer_drained);
        let failing = async { Err(io::Error::other("accept failed")) };
        let peer = async move {
            shutdown_receiver.changed().await.expect("shutdown signal");
            peer_flag.store(true, Ordering::Release);
            Ok(())
        };

        let result = coordinate_servers(
            failing,
            peer,
            future::pending(),
            shutdown_sender,
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_err());
        assert!(peer_drained.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_deadline_rejects_stuck_servers() {
        let (shutdown_sender, _shutdown_receiver) = watch::channel(false);
        let result = coordinate_servers(
            future::pending::<io::Result<()>>(),
            future::pending::<io::Result<()>>(),
            future::ready(()),
            shutdown_sender,
            Duration::from_millis(1),
        )
        .await;

        assert_eq!(
            result.expect_err("stuck servers must time out").kind(),
            io::ErrorKind::TimedOut
        );
    }
}
