use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::{AppState, auth_error_response, require_admin, require_csrf, require_session};

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn bad_req(msg: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(ErrorBody { error: msg })).into_response()
}

pub async fn get_providers(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    match state.ai.get_providers(session.user.id).await {
        Ok(list) => Json(list).into_response(),
        Err(err) => bad_req(err),
    }
}

#[derive(Deserialize)]
pub struct SaveProviderRequest {
    provider: String,
    api_key: Option<String>,
    base_url: Option<String>,
}

pub async fn put_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SaveProviderRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .ai
        .save_provider(
            session.user.id,
            &req.provider,
            req.api_key.as_deref(),
            req.base_url.as_deref(),
        )
        .await
    {
        Ok(view) => Json(view).into_response(),
        Err(err) => bad_req(err),
    }
}

pub async fn list_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    Json(state.ai.list_models(session.user.id).await).into_response()
}

#[derive(Deserialize)]
pub struct ChatRequest {
    model: String,
    message: String,
}

/// Respuesta de chat por SSE con razonamiento plegable, texto y acciones propuestas.
pub async fn post_chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    if req.message.trim().is_empty() {
        return bad_req("Escribí un mensaje antes de enviar.".to_string());
    }

    match state
        .ai
        .chat(session.user.id, &req.model, &req.message)
        .await
    {
        Ok(turn) => {
            let mut events: Vec<Result<Event, Infallible>> = Vec::new();
            if let Some(notice) = &turn.cloud_notice {
                let payload = serde_json::json!({ "type": "cloud_notice", "text": notice });
                events.push(Ok(Event::default().data(payload.to_string())));
            }
            if !turn.reasoning.is_empty() {
                let payload = serde_json::json!({ "type": "reasoning", "text": turn.reasoning });
                events.push(Ok(Event::default().data(payload.to_string())));
            }
            let text_payload = serde_json::json!({ "type": "content", "text": turn.content });
            events.push(Ok(Event::default().data(text_payload.to_string())));
            if let Some(action) = turn.proposed_action {
                let act_payload = serde_json::json!({ "type": "action", "action": action });
                events.push(Ok(Event::default().data(act_payload.to_string())));
            }
            events.push(Ok(
                Event::default().data(serde_json::json!({ "type": "done" }).to_string())
            ));

            Sse::new(futures_util::stream::iter(events)).into_response()
        }
        Err(err) => bad_req(err),
    }
}

pub async fn approve_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .ai
        .approve_action(session.user.id, session.user.role, id)
        .await
    {
        Ok(action) => Json(action).into_response(),
        Err(err) => (StatusCode::FORBIDDEN, Json(ErrorBody { error: err })).into_response(),
    }
}

pub async fn reject_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state.ai.reject_action(id).await {
        Ok(action) => Json(action).into_response(),
        Err(err) => bad_req(err),
    }
}

// --- Proyectos de agentes locales 24/7 (Antigravity / Claude) ---

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    name: String,
    runner: String,
    workdir: String,
    command: Option<String>,
    #[serde(default = "default_true")]
    auto_restart: bool,
}

fn default_true() -> bool {
    true
}

pub async fn list_projects(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    match state.ai.projects.list_projects().await {
        Ok(list) => Json(list).into_response(),
        Err(err) => bad_req(err),
    }
}

pub async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateProjectRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .ai
        .projects
        .create_project(
            &req.name,
            &req.runner,
            &req.workdir,
            req.command.as_deref(),
            req.auto_restart,
        )
        .await
    {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(err) => bad_req(err),
    }
}

pub async fn start_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state.ai.projects.start_project(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => bad_req(err),
    }
}

pub async fn stop_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    state.ai.projects.stop_project(id);
    StatusCode::NO_CONTENT.into_response()
}

pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state.ai.projects.delete_project(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => bad_req(err),
    }
}

pub async fn project_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    Json(serde_json::json!({ "lines": state.ai.projects.project_logs(id) })).into_response()
}
