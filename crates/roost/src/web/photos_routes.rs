use std::io::SeekFrom;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::photos::PhotoError;

use super::{AppState, auth_error_response, require_csrf, require_session};

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn photo_error_response(err: PhotoError) -> Response {
    let status = match &err {
        PhotoError::BadRequest(_) => StatusCode::BAD_REQUEST,
        PhotoError::NotFound(_) => StatusCode::NOT_FOUND,
        PhotoError::Forbidden(_) => StatusCode::FORBIDDEN,
        PhotoError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ErrorBody {
            error: err.to_string(),
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct ListPhotosQuery {
    cursor: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    album: Option<i64>,
    #[serde(default)]
    trash: bool,
    limit: Option<i64>,
}

pub async fn list_photos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListPhotosQuery>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    match state
        .photos
        .list_photos(
            session.user.id,
            q.cursor.as_deref(),
            q.from,
            q.to,
            q.album,
            q.trash,
            q.limit.unwrap_or(50),
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn upload_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }

    let upload_id = headers
        .get("x-upload-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let offset = headers
        .get("x-chunk-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let total_size = headers
        .get("x-total-size")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let filename = headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("foto.jpg");
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let taken_at = headers
        .get("x-taken-at")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok());

    match state
        .photos
        .upload_chunk(
            session.user.id,
            upload_id.as_deref(),
            offset,
            total_size,
            filename,
            content_type,
            taken_at,
            &body,
        )
        .await
    {
        Ok(result) => {
            let status = if result.complete {
                StatusCode::CREATED
            } else {
                StatusCode::ACCEPTED
            };
            (status, Json(result)).into_response()
        }
        Err(err) => photo_error_response(err),
    }
}

pub async fn get_photo_thumb(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, size)): Path<(i64, u32)>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    let path = match state.photos.get_thumb_path(session.user.id, id, size).await {
        Ok(p) => p,
        Err(err) => return photo_error_response(err),
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => {
            return photo_error_response(PhotoError::NotFound(
                "No pudimos abrir la miniatura.".to_string(),
            ));
        }
    };
    let ct = if path.extension().and_then(|e| e.to_str()) == Some("webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    };
    let mut resp = (StatusCode::OK, bytes).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    resp
}

pub async fn get_photo_original(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    let (photo, path) = match state.photos.get_accessible_photo(session.user.id, id).await {
        Ok(p) => p,
        Err(err) => return photo_error_response(err),
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => {
            return photo_error_response(PhotoError::NotFound(
                "No pudimos abrir el archivo original.".to_string(),
            ));
        }
    };
    let mut resp = (StatusCode::OK, bytes).into_response();
    if let Ok(val) = HeaderValue::from_str(&photo.mime_type) {
        resp.headers_mut().insert(header::CONTENT_TYPE, val);
    }
    resp
}

/// Streaming de video con soporte de rangos HTTP (`Range: bytes=start-end`) sin cargar todo en RAM.
pub async fn stream_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    let (photo, path) = match state.photos.get_accessible_photo(session.user.id, id).await {
        Ok(p) => p,
        Err(err) => return photo_error_response(err),
    };

    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => {
            return photo_error_response(PhotoError::NotFound(
                "No pudimos abrir el video.".to_string(),
            ));
        }
    };
    let file_len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    const MAX_CHUNK: u64 = 1024 * 1024; // 1 MiB por bloque para mantener la RAM muy baja.
    let (start, end, partial) =
        if let Some(range_hdr) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
            if let Some(spec) = range_hdr.strip_prefix("bytes=") {
                let mut parts = spec.split('-');
                let s = parts
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                let e = parts
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or_else(|| (s + MAX_CHUNK - 1).min(file_len.saturating_sub(1)));
                if s >= file_len || s > e {
                    return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                }
                (
                    s,
                    e.min(file_len.saturating_sub(1)).min(s + MAX_CHUNK - 1),
                    true,
                )
            } else {
                (0, file_len.saturating_sub(1).min(MAX_CHUNK - 1), false)
            }
        } else {
            (0, file_len.saturating_sub(1).min(MAX_CHUNK - 1), false)
        };

    let len = if file_len == 0 { 0 } else { end - start + 1 };
    let mut buf = vec![0u8; len as usize];
    if len > 0
        && (file.seek(SeekFrom::Start(start)).await.is_err()
            || file.read_exact(&mut buf).await.is_err())
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let status = if partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut resp = (status, buf).into_response();
    if let Ok(val) = HeaderValue::from_str(&photo.mime_type) {
        resp.headers_mut().insert(header::CONTENT_TYPE, val);
    }
    resp.headers_mut()
        .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if partial && let Ok(cr) = HeaderValue::from_str(&format!("bytes {start}-{end}/{file_len}")) {
        resp.headers_mut().insert(header::CONTENT_RANGE, cr);
    }
    resp
}

pub async fn delete_photo(
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
    match state.photos.move_to_trash(session.user.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn restore_photo(
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
    match state.photos.restore_photo(session.user.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}

#[derive(Deserialize)]
pub struct AlbumNameRequest {
    name: String,
}

#[derive(Deserialize)]
pub struct ShareAlbumRequest {
    user_id: i64,
    #[serde(default)]
    can_edit: bool,
}

#[derive(Deserialize)]
pub struct AddAlbumPhotoRequest {
    photo_id: i64,
}

pub async fn list_albums(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    match state.photos.list_albums(session.user.id).await {
        Ok(albums) => Json(albums).into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn create_album(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AlbumNameRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state.photos.create_album(session.user.id, &req.name).await {
        Ok(album) => (StatusCode::CREATED, Json(album)).into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn patch_album(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<AlbumNameRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .photos
        .rename_album(session.user.id, id, &req.name)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn delete_album(
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
    match state.photos.delete_album(session.user.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn share_album(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<ShareAlbumRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .photos
        .share_album(session.user.id, id, req.user_id, req.can_edit)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}

pub async fn add_album_photo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<AddAlbumPhotoRequest>,
) -> Response {
    let session = match require_session(&state, &headers).await {
        Ok(s) => s,
        Err(err) => return auth_error_response(err),
    };
    if let Err(err) = require_csrf(&session, &headers) {
        return auth_error_response(err);
    }
    match state
        .photos
        .add_photo_to_album(session.user.id, id, req.photo_id)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => photo_error_response(err),
    }
}
