use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use roost::apps::AppService;
use roost::docker::{BollardDocker, DockerRuntime, MockDocker};
use roost::web::AppState;
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

    let apps_dir = std::env::var("ROOST_APPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("apps"));
    let data_dir = std::env::var("ROOST_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/roost"));

    let docker: Arc<dyn DockerRuntime> = if std::env::var("ROOST_DEMO").as_deref() == Ok("1") {
        tracing::info!("Iniciando Roost en modo simulado (ROOST_DEMO=1)");
        Arc::new(MockDocker::new())
    } else {
        match BollardDocker::connect_local() {
            Ok(client) => Arc::new(client),
            Err(err) => {
                tracing::warn!(
                    "No pudimos conectar con el motor local ({err}); continuamos en modo simulado."
                );
                Arc::new(MockDocker::new())
            }
        }
    };

    // En M5 se reemplaza por lectura real de memoria disponible con `sysinfo`.
    let ram_provider = Arc::new(|| 4096_u64);

    let apps_service = match AppService::from_dir(
        &apps_dir,
        data_dir.clone(),
        docker.clone(),
        ram_provider.clone(),
    ) {
        Ok(service) => service,
        Err(err) => {
            tracing::warn!(
                "No se pudo cargar el catálogo desde '{}': {err}. Arrancando con catálogo vacío.",
                apps_dir.display()
            );
            AppService::new(Vec::new(), HashMap::new(), data_dir, docker, ram_provider)
        }
    };

    let state = AppState {
        apps: Arc::new(apps_service),
    };

    let addr = bind_addr();
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!("No pudimos iniciar Roost en {addr}: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!("Roost escuchando en http://{addr}");

    if let Err(err) = axum::serve(listener, web::router(state))
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
