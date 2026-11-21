use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use crate::apps::{AppError, AppService};

/// Estado compartido del servidor HTTP de Roost.
#[derive(Clone)]
pub struct AppState {
    pub apps: Arc<AppService>,
}

/// Construye el router HTTP principal de Roost con el estado provisto.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/store", get(get_store))
        .route("/api/apps", get(get_installed_apps))
        .route("/api/apps/{id}/install", post(install_app))
        .route("/api/apps/{id}/start", post(start_app))
        .route("/api/apps/{id}/stop", post(stop_app))
        .route("/api/apps/{id}/restart", post(restart_app))
        .route("/api/apps/{id}", delete(uninstall_app))
        .route("/api/events", get(events_sse))
        .with_state(state)
}

async fn index() -> &'static str {
    "Roost"
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Serialize)]
struct ActionResponse {
    app_id: String,
    status: String,
}

#[derive(Deserialize)]
struct UninstallRequest {
    #[serde(default)]
    confirm: bool,
}

fn app_error_response(err: AppError) -> Response {
    let status = match &err {
        AppError::NotFound(_) => StatusCode::NOT_FOUND,
        AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
        AppError::RuntimeUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ErrorBody {
            error: err.to_string(),
        }),
    )
        .into_response()
}

async fn get_store(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.apps.store().to_vec())
}

async fn get_installed_apps(State(state): State<AppState>) -> Response {
    match state.apps.installed_apps().await {
        Ok(apps) => Json(apps).into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn install_app(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.apps.install(&id) {
        Ok(resp) => (StatusCode::ACCEPTED, Json(resp)).into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn start_app(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.apps.start(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "running".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn stop_app(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.apps.stop(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "stopped".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn restart_app(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.apps.restart(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "running".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn uninstall_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<UninstallRequest>>,
) -> Response {
    let confirmed = body.map(|Json(req)| req.confirm).unwrap_or(false);
    if !confirmed {
        return app_error_response(AppError::BadRequest(
            "Para desinstalar esta app tenés que confirmarlo enviando confirm: true.".to_string(),
        ));
    }

    match state.apps.uninstall(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "uninstalled".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn events_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.apps.subscribe();

    let stream = futures_util::stream::unfold(rx, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        return Some((Ok(Event::default().data(json)), receiver));
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Method, Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::apps::RoostEvent;
    use crate::docker::MockDocker;

    fn test_state(available_ram_mb: u64) -> AppState {
        let apps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let docker = Arc::new(MockDocker::new());
        let service = AppService::from_dir(
            &apps_dir,
            PathBuf::from("/tmp/roost-test"),
            docker,
            Arc::new(move || available_ram_mb),
        )
        .expect("el catálogo de prueba debe cargar");

        AppState {
            apps: Arc::new(service),
        }
    }

    #[tokio::test]
    async fn root_returns_roost() {
        let app = router(test_state(4096));

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"Roost");
    }

    #[tokio::test]
    async fn store_lists_all_five_catalog_apps() {
        let app = router(test_state(4096));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/store")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let items: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(items.as_array().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn install_emits_live_progress_and_ram_warning_when_low() {
        let state = test_state(512);
        let mut rx = state.apps.subscribe();
        let app = router(state.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/install")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["app_id"], "jellyfin");
        assert_eq!(body["status"], "installing");
        assert!(body["ram_warning"].as_str().unwrap().contains("1024 MB"));

        let mut saw_progress = false;
        let mut saw_finished = false;
        for _ in 0..16 {
            let ev = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("debería recibir evento SSE")
                .expect("canal abierto");
            match ev {
                RoostEvent::AppInstallProgress {
                    app_id,
                    downloaded_mb,
                    ..
                } if app_id == "jellyfin" && downloaded_mb > 0.0 => {
                    saw_progress = true;
                }
                RoostEvent::AppInstallFinished { app_id, .. } if app_id == "jellyfin" => {
                    saw_finished = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_progress, "debe emitir progreso en MB");
        assert!(saw_finished, "debe emitir finalización");

        // Verificar ciclo de vida completo: listar, detener, iniciar, reiniciar y desinstalar.
        let installed = state.apps.installed_apps().await.unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].id, "jellyfin");

        let app = router(state.clone());
        let stop_res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stop_res.status(), StatusCode::OK);

        let app = router(state.clone());
        let start_res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_res.status(), StatusCode::OK);

        let app = router(state.clone());
        let restart_res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/restart")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(restart_res.status(), StatusCode::OK);

        // Sin confirm: true debe rechazar la desinstalación.
        let app = router(state.clone());
        let bad_del = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/apps/jellyfin")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"confirm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_del.status(), StatusCode::BAD_REQUEST);

        // Con confirm: true desinstala la app.
        let app = router(state.clone());
        let ok_del = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/apps/jellyfin")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"confirm":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_del.status(), StatusCode::OK);
        assert!(state.apps.installed_apps().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sse_endpoint_returns_event_stream_headers() {
        let app = router(test_state(4096));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let ct = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("text/event-stream"));
    }
}
