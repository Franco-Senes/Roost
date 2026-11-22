use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use roost::ai::AiService;
use roost::ai::crypto::SecretCipher;
use roost::apps::AppService;
use roost::auth::AuthService;
use roost::db::Database;
use roost::docker::{BollardDocker, DockerRuntime, MockDocker};
use roost::jobs::JobQueue;
use roost::market::MarketService;
use roost::metrics::MetricsService;
use roost::photos::PhotoService;
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
    let secure_cookies = std::env::var("ROOST_HTTPS").as_deref() == Ok("1");

    let db = if std::env::var("ROOST_DEMO").as_deref() == Ok("1") {
        match Database::connect_in_memory().await {
            Ok(db) => db,
            Err(err) => {
                tracing::error!("No pudimos iniciar la base de datos temporal: {err}");
                std::process::exit(1);
            }
        }
    } else {
        let db_path = data_dir.join("roost.db");
        match Database::connect_file(&db_path).await {
            Ok(db) => db,
            Err(err) => {
                tracing::error!(
                    "No pudimos abrir la base de datos en '{}': {err}",
                    db_path.display()
                );
                std::process::exit(1);
            }
        }
    };

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

    let demo_mode = std::env::var("ROOST_DEMO").as_deref() == Ok("1");
    let metrics = MetricsService::new(demo_mode);
    let ram_metrics = metrics.clone();
    let ram_provider = Arc::new(move || ram_metrics.available_ram_mb());

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
            AppService::new(
                Vec::new(),
                HashMap::new(),
                data_dir.clone(),
                docker,
                ram_provider,
            )
        }
    };

    // Solo publica métricas cuando hay al menos un panel abierto en /api/events.
    metrics.spawn_lazy_publisher(apps_service.event_sender(), Duration::from_secs(2));

    let photo_data_dir = if demo_mode {
        std::env::temp_dir().join("roost-demo-data")
    } else {
        data_dir
    };
    let jobs = JobQueue::new(&db);
    let photos = PhotoService::new(&db, photo_data_dir.clone(), jobs.clone());
    let worker_photos = photos.clone();
    jobs.spawn_worker(metrics.clone(), move |job| {
        let svc = worker_photos.clone();
        async move {
            if job.kind == "photo_thumb"
                && let Ok(photo_id) = job.payload.parse::<i64>()
            {
                svc.generate_thumbnails(photo_id).await?;
            }
            Ok(())
        }
    });

    let apps_arc = Arc::new(apps_service);
    let sync_apps = Arc::clone(&apps_arc);
    tokio::spawn(async move {
        match sync_apps.sync_remote_catalog().await {
            Ok(added) => tracing::info!("Catálogo online sincronizado (+{added} apps reales)"),
            Err(err) => tracing::warn!("No se pudo sincronizar el catálogo online: {err}"),
        }
    });

    let cipher = if demo_mode {
        SecretCipher::ephemeral()
    } else {
        SecretCipher::load_or_create(&photo_data_dir.join("secret.key"))
            .unwrap_or_else(|_| SecretCipher::ephemeral())
    };
    let ai = AiService::new(
        &db,
        cipher,
        Arc::clone(&apps_arc),
        metrics.clone(),
        photos.clone(),
    );

    let skills_dir = std::env::var("ROOST_SKILLS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("skills"));
    let market = MarketService::from_dir(&db, &skills_dir);

    let state = AppState {
        apps: apps_arc,
        auth: Arc::new(AuthService::new(&db)),
        ai,
        market,
        db,
        metrics,
        photos,
        demo_mode,
        secure_cookies,
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
