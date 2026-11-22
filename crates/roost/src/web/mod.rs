mod ai_routes;
mod photos_routes;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tower_http::compression::CompressionLayer;

use crate::ai::AiService;
use crate::apps::{AppError, AppService};
use crate::auth::{AuthError, AuthService, Role, SESSION_TTL_SECS, SessionInfo, constant_time_eq};
use crate::db::Database;
use crate::metrics::MetricsService;
use crate::photos::PhotoService;

const SESSION_COOKIE: &str = "roost_session";
const CSRF_HEADER: &str = "x-csrf-token";

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct WebAssets;

/// Estado compartido del servidor HTTP de Roost.
#[derive(Clone)]
pub struct AppState {
    pub apps: Arc<AppService>,
    pub auth: Arc<AuthService>,
    pub ai: AiService,
    pub db: Database,
    pub metrics: MetricsService,
    pub photos: PhotoService,
    pub demo_mode: bool,
    pub secure_cookies: bool,
}

/// Construye el router HTTP principal de Roost con el estado provisto.
pub fn router(state: AppState) -> Router {
    Router::new()
        // Primer arranque y sesión (públicas)
        .route("/api/setup", get(get_setup).post(post_setup))
        .route("/api/login", post(post_login))
        // Sesión y perfil (requieren sesión)
        .route("/api/logout", post(post_logout))
        .route("/api/me", get(get_me))
        // Gestión de usuarios (admin)
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/{id}", patch(update_user).delete(delete_user))
        // Ajustes
        .route("/api/settings", get(get_settings).patch(patch_settings))
        // Sistema y métricas
        .route("/api/system", get(get_system))
        // Tienda, apps y eventos
        .route("/api/store", get(get_store))
        .route("/api/store/refresh", post(refresh_store))
        .route("/api/store/{id}/icon", get(get_store_icon))
        .route("/api/apps", get(get_installed_apps))
        .route("/api/apps/{id}/install", post(install_app))
        .route("/api/apps/{id}/start", post(start_app))
        .route("/api/apps/{id}/stop", post(stop_app))
        .route("/api/apps/{id}/restart", post(restart_app))
        .route("/api/apps/{id}", delete(uninstall_app))
        .route("/api/events", get(events_sse))
        // Fotos, videos y álbumes (M7)
        .route(
            "/api/photos",
            get(photos_routes::list_photos).post(photos_routes::upload_photo),
        )
        .route(
            "/api/photos/{id}/thumb/{size}",
            get(photos_routes::get_photo_thumb),
        )
        .route(
            "/api/photos/{id}/original",
            get(photos_routes::get_photo_original),
        )
        .route("/api/photos/{id}/stream", get(photos_routes::stream_photo))
        .route("/api/photos/{id}", delete(photos_routes::delete_photo))
        .route(
            "/api/photos/{id}/restore",
            post(photos_routes::restore_photo),
        )
        .route(
            "/api/albums",
            get(photos_routes::list_albums).post(photos_routes::create_album),
        )
        .route(
            "/api/albums/{id}",
            patch(photos_routes::patch_album).delete(photos_routes::delete_album),
        )
        .route("/api/albums/{id}/share", post(photos_routes::share_album))
        .route(
            "/api/albums/{id}/photos",
            post(photos_routes::add_album_photo),
        )
        // Agente de IA y proyectos locales 24/7 (M9)
        .route(
            "/api/ai/providers",
            get(ai_routes::get_providers).put(ai_routes::put_provider),
        )
        .route("/api/ai/models", get(ai_routes::list_models))
        .route("/api/ai/chat", post(ai_routes::post_chat))
        .route(
            "/api/ai/actions/{id}/approve",
            post(ai_routes::approve_action),
        )
        .route(
            "/api/ai/actions/{id}/reject",
            post(ai_routes::reject_action),
        )
        .route(
            "/api/ai/projects",
            get(ai_routes::list_projects).post(ai_routes::create_project),
        )
        .route(
            "/api/ai/projects/{id}/start",
            post(ai_routes::start_project),
        )
        .route("/api/ai/projects/{id}/stop", post(ai_routes::stop_project))
        .route("/api/ai/projects/{id}", delete(ai_routes::delete_project))
        .route("/api/ai/projects/{id}/logs", get(ai_routes::project_logs))
        // Interfaz Material 3 embebida (SPA fallback)
        .fallback(serve_web_asset)
        .layer(CompressionLayer::new())
        .with_state(state)
}

fn content_type_for(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".webmanifest") || path.ends_with(".json") {
        "application/manifest+json; charset=utf-8"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

async fn serve_web_asset(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');
    let path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    let (asset_path, file) = match WebAssets::get(path) {
        Some(f) => (path, f),
        None => match WebAssets::get("index.html") {
            Some(f) => ("index.html", f),
            None => return (StatusCode::OK, "Roost").into_response(),
        },
    };

    let mut resp = (StatusCode::OK, file.data.into_owned()).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type_for(asset_path)),
    );
    if asset_path.starts_with("assets/") {
        resp.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }
    resp
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
struct ConfirmRequest {
    #[serde(default)]
    confirm: bool,
}

#[derive(Serialize)]
struct SetupStatusResponse {
    needs_setup: bool,
}

#[derive(Deserialize)]
struct CredentialsRequest {
    name: String,
    password: String,
}

#[derive(Serialize)]
struct MeResponse {
    id: i64,
    name: String,
    role: Role,
    csrf_token: String,
}

#[derive(Deserialize)]
struct CreateUserRequest {
    name: String,
    password: String,
    #[serde(default = "default_member_role")]
    role: Role,
}

fn default_member_role() -> Role {
    Role::Member
}

#[derive(Deserialize)]
struct UpdateUserRequest {
    role: Option<Role>,
    suspended: Option<bool>,
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

fn auth_error_response(err: AuthError) -> Response {
    let status = match &err {
        AuthError::BadRequest(_) => StatusCode::BAD_REQUEST,
        AuthError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
        AuthError::Forbidden(_) => StatusCode::FORBIDDEN,
        AuthError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
        AuthError::Conflict(_) => StatusCode::CONFLICT,
        AuthError::NotFound(_) => StatusCode::NOT_FOUND,
        AuthError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ErrorBody {
            error: err.to_string(),
        }),
    )
        .into_response()
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let trimmed = pair.trim();
        if let Some((k, v)) = trimmed.split_once('=')
            && k.trim() == name
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn build_session_cookie(session_id: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={session_id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={SESSION_TTL_SECS}{secure_flag}"
    )
}

fn clear_session_cookie(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure_flag}")
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("local")
        .to_string()
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

async fn require_session(state: &AppState, headers: &HeaderMap) -> Result<SessionInfo, AuthError> {
    let Some(sid) = read_cookie(headers, SESSION_COOKIE) else {
        return Err(AuthError::Unauthorized(
            "Tenés que iniciar sesión para continuar.".to_string(),
        ));
    };
    state.auth.verify_session(&sid).await
}

fn require_admin(session: &SessionInfo) -> Result<(), AuthError> {
    if session.user.role != Role::Admin {
        return Err(AuthError::Forbidden(
            "Solo el administrador puede hacer esto.".to_string(),
        ));
    }
    Ok(())
}

fn require_csrf(session: &SessionInfo, headers: &HeaderMap) -> Result<(), AuthError> {
    let provided = headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !constant_time_eq(provided, &session.csrf_token) {
        return Err(AuthError::Forbidden(
            "Falta el código de seguridad de la sesión. Recargá la página y probá de nuevo."
                .to_string(),
        ));
    }
    Ok(())
}

fn session_response(status: StatusCode, session: SessionInfo, secure_cookies: bool) -> Response {
    let mut resp = (
        status,
        Json(MeResponse {
            id: session.user.id,
            name: session.user.name,
            role: session.user.role,
            csrf_token: session.csrf_token,
        }),
    )
        .into_response();

    if let Ok(val) =
        HeaderValue::from_str(&build_session_cookie(&session.session_id, secure_cookies))
    {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn get_setup(State(state): State<AppState>) -> Response {
    match state.auth.needs_setup().await {
        Ok(needs_setup) => Json(SetupStatusResponse { needs_setup }).into_response(),
        Err(err) => auth_error_response(err),
    }
}

async fn post_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CredentialsRequest>,
) -> Response {
    let ua = user_agent(&headers);
    match state.auth.setup_admin(&req.name, &req.password, &ua).await {
        Ok(session) => {
            if state.demo_mode {
                let _ = state.photos.seed_demo_gallery(session.user.id).await;
            }
            session_response(StatusCode::CREATED, session, state.secure_cookies)
        }
        Err(err) => auth_error_response(err),
    }
}

async fn post_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CredentialsRequest>,
) -> Response {
    let ip = client_ip(&headers);
    let ua = user_agent(&headers);
    match state.auth.login(&ip, &req.name, &req.password, &ua).await {
        Ok(session) => session_response(StatusCode::OK, session, state.secure_cookies),
        Err(err) => auth_error_response(err),
    }
}

async fn post_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }

    if let Err(err) = state.auth.logout(&session.session_id).await {
        return auth_error_response(err);
    }

    let mut resp = StatusCode::NO_CONTENT.into_response();
    if let Ok(val) = HeaderValue::from_str(&clear_session_cookie(state.secure_cookies)) {
        resp.headers_mut().append(header::SET_COOKIE, val);
    }
    resp
}

async fn get_me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    Json(MeResponse {
        id: session.user.id,
        name: session.user.name,
        role: session.user.role,
        csrf_token: session.csrf_token,
    })
    .into_response()
}

async fn list_users(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_admin(&session) {
        return auth_error_response(err);
    }
    match state.auth.list_users().await {
        Ok(users) => Json(users).into_response(),
        Err(err) => auth_error_response(err),
    }
}

async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
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
        .auth
        .create_user(&req.name, &req.password, req.role)
        .await
    {
        Ok(user) => (StatusCode::CREATED, Json(user)).into_response(),
        Err(err) => auth_error_response(err),
    }
}

async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
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
        .auth
        .update_user(session.user.id, target_id, req.role, req.suspended)
        .await
    {
        Ok(user) => Json(user).into_response(),
        Err(err) => auth_error_response(err),
    }
}

async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target_id): Path<i64>,
    body: Option<Json<ConfirmRequest>>,
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
    let confirmed = body.map(|Json(req)| req.confirm).unwrap_or(false);
    if !confirmed {
        return auth_error_response(AuthError::BadRequest(
            "Para eliminar un usuario tenés que confirmarlo enviando confirm: true.".to_string(),
        ));
    }
    match state.auth.delete_user(session.user.id, target_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => auth_error_response(err),
    }
}

async fn get_store(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }
    Json(state.apps.store()).into_response()
}

async fn refresh_store(State(state): State<AppState>, headers: HeaderMap) -> Response {
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
    match state.apps.sync_remote_catalog().await {
        Ok(added) => Json(serde_json::json!({
            "added": added,
            "total": state.apps.store().len(),
        }))
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn get_store_icon(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }
    match state.apps.icon_svg(&id) {
        Some(svg) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/svg+xml")],
            svg.to_string(),
        )
            .into_response(),
        None => app_error_response(AppError::NotFound(
            "No encontramos el ícono de esa app.".to_string(),
        )),
    }
}

#[derive(Deserialize)]
struct PatchSettingsRequest {
    theme: Option<String>,
    mode: Option<String>,
    server_name: Option<String>,
}

async fn get_settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }
    match state.db.get_settings().await {
        Ok(mut map) => {
            map.entry("theme".to_string())
                .or_insert_with(|| "green".to_string());
            map.entry("mode".to_string())
                .or_insert_with(|| "light".to_string());
            map.entry("server_name".to_string())
                .or_insert_with(|| "Roost".to_string());
            Json(map).into_response()
        }
        Err(_) => auth_error_response(AuthError::Internal(
            "No pudimos leer los ajustes.".to_string(),
        )),
    }
}

async fn patch_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PatchSettingsRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }

    if let Some(theme) = req.theme {
        if !matches!(theme.as_str(), "green" | "blue" | "purple") {
            return auth_error_response(AuthError::BadRequest(
                "Tema inválido. Elegí Verde, Azul o Violeta.".to_string(),
            ));
        }
        if state.db.set_setting("theme", &theme).await.is_err() {
            return auth_error_response(AuthError::Internal(
                "No pudimos guardar el tema.".to_string(),
            ));
        }
    }

    if let Some(mode) = req.mode {
        if !matches!(mode.as_str(), "light" | "dark") {
            return auth_error_response(AuthError::BadRequest(
                "Modo inválido. Elegí claro u oscuro.".to_string(),
            ));
        }
        if state.db.set_setting("mode", &mode).await.is_err() {
            return auth_error_response(AuthError::Internal(
                "No pudimos guardar el modo.".to_string(),
            ));
        }
    }

    if let Some(name) = req.server_name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return auth_error_response(AuthError::BadRequest(
                "El nombre del servidor debe tener entre 1 y 64 caracteres.".to_string(),
            ));
        }
        if state.db.set_setting("server_name", trimmed).await.is_err() {
            return auth_error_response(AuthError::Internal(
                "No pudimos guardar el nombre del servidor.".to_string(),
            ));
        }
    }

    get_settings(State(state), headers).await
}

async fn get_installed_apps(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }
    match state.apps.installed_apps().await {
        Ok(apps) => Json(apps).into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn install_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match state.apps.install(&id) {
        Ok(resp) => (StatusCode::ACCEPTED, Json(resp)).into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn start_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match state.apps.start(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "running".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn stop_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    match state.apps.stop(&id).await {
        Ok(()) => Json(ActionResponse {
            app_id: id,
            status: "stopped".to_string(),
        })
        .into_response(),
        Err(err) => app_error_response(err),
    }
}

async fn restart_app(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
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
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Option<Json<ConfirmRequest>>,
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

async fn get_system(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }
    Json(state.metrics.snapshot()).into_response()
}

async fn events_sse(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(err) = require_session(&state, &headers).await {
        return auth_error_response(err);
    }

    let rx = state.apps.subscribe();
    let viewer_guard = state.metrics.register_viewer();

    let stream =
        futures_util::stream::unfold((rx, viewer_guard), |(mut receiver, guard)| async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            return Some((
                                Ok::<Event, Infallible>(Event::default().data(json)),
                                (receiver, guard),
                            ));
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return None,
                }
            }
        });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::apps::RoostEvent;
    use crate::db::Database;
    use crate::docker::MockDocker;

    async fn test_state(available_ram_mb: u64) -> AppState {
        let apps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let docker = Arc::new(MockDocker::new());
        let service = AppService::from_dir(
            &apps_dir,
            PathBuf::from("/tmp/roost-test"),
            docker,
            Arc::new(move || available_ram_mb),
        )
        .expect("el catálogo de prueba debe cargar");

        let db = Database::connect_in_memory().await.unwrap();
        let auth = Arc::new(AuthService::new(&db));
        let jobs = crate::jobs::JobQueue::new(&db);
        let photos = PhotoService::new(&db, std::env::temp_dir().join("roost-web-test"), jobs);
        let apps = Arc::new(service);
        let metrics = MetricsService::new(true);
        let ai = AiService::new(
            &db,
            crate::ai::crypto::SecretCipher::ephemeral(),
            Arc::clone(&apps),
            metrics.clone(),
            photos.clone(),
        );

        AppState {
            apps,
            auth,
            ai,
            db,
            metrics,
            photos,
            demo_mode: false,
            secure_cookies: false,
        }
    }

    /// Crea el admin inicial y devuelve `(cookie_header, csrf_token)`.
    async fn setup_admin_session(state: &AppState) -> (String, String) {
        let app = router(state.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/setup")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Franco","password":"contrasena-admin-2026"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        let cookie_pair = set_cookie.split(';').next().unwrap().to_string();

        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let csrf = body["csrf_token"].as_str().unwrap().to_string();
        (cookie_pair, csrf)
    }

    #[tokio::test]
    async fn root_returns_roost_and_api_requires_session() {
        let state = test_state(4096).await;

        let res = router(state.clone())
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(html.contains("Roost"));

        let unauth = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/store")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn member_cannot_install_apps_or_manage_users() {
        let state = test_state(4096).await;
        let (admin_cookie, admin_csrf) = setup_admin_session(&state).await;

        // El admin crea una cuenta de miembro.
        let create_res = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/users")
                    .header(header::COOKIE, &admin_cookie)
                    .header(CSRF_HEADER, &admin_csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Sofi","password":"clave-miembro-2026","role":"member"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);

        // El miembro inicia sesión.
        let login_res = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"name":"Sofi","password":"clave-miembro-2026"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_res.status(), StatusCode::OK);
        let member_cookie = login_res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        let bytes = login_res.into_body().collect().await.unwrap().to_bytes();
        let member_csrf =
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["csrf_token"]
                .as_str()
                .unwrap()
                .to_string();

        // El miembro puede ver la Tienda...
        let store_res = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/store")
                    .header(header::COOKIE, &member_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(store_res.status(), StatusCode::OK);

        // ...pero NO puede instalar apps ni listar usuarios.
        let install_res = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/install")
                    .header(header::COOKIE, &member_cookie)
                    .header(CSRF_HEADER, &member_csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(install_res.status(), StatusCode::FORBIDDEN);

        let users_res = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/users")
                    .header(header::COOKIE, &member_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(users_res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn csrf_protection_blocks_mutations_without_valid_token() {
        let state = test_state(4096).await;
        let (admin_cookie, _admin_csrf) = setup_admin_session(&state).await;

        let res = router(state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/install")
                    .header(header::COOKIE, &admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_install_emits_live_progress_and_ram_warning_when_low() {
        let state = test_state(512).await;
        let (admin_cookie, admin_csrf) = setup_admin_session(&state).await;
        let mut rx = state.apps.subscribe();

        let response = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/apps/jellyfin/install")
                    .header(header::COOKIE, &admin_cookie)
                    .header(CSRF_HEADER, &admin_csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
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
        assert!(saw_progress);
        assert!(saw_finished);

        // Desinstalar sin confirm: true devuelve 400; con confirm: true funciona.
        let bad_del = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/apps/jellyfin")
                    .header(header::COOKIE, &admin_cookie)
                    .header(CSRF_HEADER, &admin_csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"confirm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_del.status(), StatusCode::BAD_REQUEST);

        let ok_del = router(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/apps/jellyfin")
                    .header(header::COOKIE, &admin_cookie)
                    .header(CSRF_HEADER, &admin_csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"confirm":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_del.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn sse_endpoint_requires_session_and_returns_event_stream() {
        let state = test_state(4096).await;
        let (admin_cookie, _) = setup_admin_session(&state).await;

        let response = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .header(header::COOKIE, &admin_cookie)
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

    #[tokio::test]
    async fn system_endpoint_requires_session_and_returns_metrics() {
        let state = test_state(4096).await;

        let unauth = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/system")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        let (admin_cookie, _) = setup_admin_session(&state).await;
        let ok = router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/system")
                    .header(header::COOKIE, &admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let bytes = ok.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["ram_total_mb"].as_u64().unwrap() > 0);
        assert_eq!(body["demo"], true);
    }

    #[tokio::test]
    async fn settings_and_store_icons_work_with_session() {
        let state = test_state(4096).await;
        let (admin_cookie, admin_csrf) = setup_admin_session(&state).await;

        let icon = router(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/api/store/jellyfin/icon")
                    .header(header::COOKIE, &admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(icon.status(), StatusCode::OK);

        let patch = router(state)
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri("/api/settings")
                    .header(header::COOKIE, &admin_cookie)
                    .header(CSRF_HEADER, &admin_csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"theme":"purple","mode":"dark"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch.status(), StatusCode::OK);
        let bytes = patch.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["theme"], "purple");
        assert_eq!(body["mode"], "dark");
    }
}
