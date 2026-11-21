use roost::{bind_addr, web};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let addr = bind_addr();
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!("No pudimos iniciar Roost en {addr}: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!("Roost escuchando en http://{addr}");

    if let Err(err) = axum::serve(listener, web::router())
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!("El servidor se detuvo con un error: {err}");
        std::process::exit(1);
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!("No pudimos escuchar la señal Ctrl+C: {err}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(err) => {
                tracing::warn!("No pudimos escuchar SIGTERM: {err}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("Apagando Roost...");
}
