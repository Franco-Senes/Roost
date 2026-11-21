use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::db::Database;
use crate::jobs::JobQueue;

/// Retención de la papelera en segundos: 30 días (`AGENTS.md` §8).
pub const TRASH_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;

/// Metadatos públicos de una foto o video en la galería.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Photo {
    pub id: i64,
    pub owner_id: i64,
    pub hash: String,
    pub filename: String,
    pub mime_type: String,
    pub kind: String,
    pub taken_at: i64,
    pub width: i64,
    pub height: i64,
    pub duration: i64,
    pub size: i64,
    pub avg_color: String,
    pub deleted_at: Option<i64>,
}

/// Respuesta paginada por cursor (`GET /api/photos`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoPage {
    pub items: Vec<Photo>,
    pub next_cursor: Option<String>,
}

/// Respuesta de una subida por partes o completa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResult {
    pub complete: bool,
    pub duplicate: bool,
    pub received_bytes: u64,
    pub photo: Option<Photo>,
}

/// Álbum propio o compartido con la persona conectada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: i64,
    pub owner_id: i64,
    pub name: String,
    pub created_at: i64,
    pub shared: bool,
    pub can_edit: bool,
    pub photo_count: i64,
}

/// Errores del módulo de fotos en español simple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoError {
    BadRequest(String),
    NotFound(String),
    Forbidden(String),
    Internal(String),
}

impl fmt::Display for PhotoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m) | Self::NotFound(m) | Self::Forbidden(m) | Self::Internal(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for PhotoError {}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn sanitize_filename(raw: &str) -> String {
    let base = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archivo.bin");
    let clean: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(80)
        .collect();
    if clean.is_empty() {
        "archivo.bin".to_string()
    } else {
        clean
    }
}

fn sanitize_upload_id(raw: &str) -> Result<String, PhotoError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(PhotoError::BadRequest(
            "Identificador de subida inválido.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn detect_kind_and_mime(filename: &str, content_type: Option<&str>) -> (&'static str, String) {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => ("video", "video/mp4".to_string()),
        "webm" => ("video", "video/webm".to_string()),
        "mov" => ("video", "video/quicktime".to_string()),
        "png" => ("photo", "image/png".to_string()),
        "webp" => ("photo", "image/webp".to_string()),
        "jpg" | "jpeg" => ("photo", "image/jpeg".to_string()),
        _ => {
            if let Some(ct) = content_type {
                if ct.starts_with("video/") {
                    return ("video", ct.to_string());
                }
                if ct.starts_with("image/") {
                    return ("photo", ct.to_string());
                }
            }
            ("photo", "image/jpeg".to_string())
        }
    }
}

/// Calcula año y mes (UTC) a partir de un timestamp Unix sin dependencias externas.
fn year_month_from_unix(secs: i64) -> (i32, u32) {
    let days = secs.div_euclid(86_400);
    // Algoritmo de Howard Hinnant (civil_from_days).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + i64::from(m <= 2)) as i32;
    (year, m as u32)
}

/// Intenta extraer la fecha EXIF (`DateTimeOriginal`) de una imagen; si falla devuelve `None`.
fn read_exif_taken_at(path: &Path) -> Option<i64> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    let field = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let raw = field.display_value().to_string();
    parse_exif_datetime(&raw)
}

fn parse_exif_datetime(raw: &str) -> Option<i64> {
    // Formato típico EXIF: "2026-07-15 18:30:00" o "2026:07:15 18:30:00"
    let normalized = raw.trim().replace(':', "-");
    let mut parts = normalized.split_whitespace();
    let date = parts.next()?;
    let time = parts.next().unwrap_or("00-00-00");
    let d: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    let t: Vec<i64> = time.split('-').filter_map(|p| p.parse().ok()).collect();
    if d.len() < 3 {
        return None;
    }
    let (year, month, day) = (d[0], d[1], d[2]);
    let (hour, min, sec) = (
        *t.first().unwrap_or(&0),
        *t.get(1).unwrap_or(&0),
        *t.get(2).unwrap_or(&0),
    );
    // Algoritmo inverso de Howard Hinnant (days_from_civil).
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + min * 60 + sec)
}

/// Calcula el hash `blake3` leyendo el archivo por bloques de 64 KiB (nunca entero en RAM).
fn hash_file_streaming(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn row_to_photo(row: &sqlx::sqlite::SqliteRow) -> Photo {
    Photo {
        id: row.get("id"),
        owner_id: row.get("owner_id"),
        hash: row.get("hash"),
        filename: row.get("filename"),
        mime_type: row.get("mime_type"),
        kind: row.get("kind"),
        taken_at: row.get("taken_at"),
        width: row.get("width"),
        height: row.get("height"),
        duration: row.get("duration"),
        size: row.get("size"),
        avg_color: row.get("avg_color"),
        deleted_at: row.get("deleted_at"),
    }
}

/// Servicio principal de fotos, videos, álbumes y miniaturas.
#[derive(Clone)]
pub struct PhotoService {
    pool: SqlitePool,
    data_dir: PathBuf,
    jobs: JobQueue,
}

impl PhotoService {
    pub fn new(db: &Database, data_dir: PathBuf, jobs: JobQueue) -> Self {
        Self {
            pool: db.pool().clone(),
            data_dir,
            jobs,
        }
    }

    fn thumbs_root(&self) -> PathBuf {
        self.data_dir.join("photos").join("thumbs")
    }

    fn uploads_root(&self) -> PathBuf {
        self.data_dir.join("photos").join("uploads")
    }

    /// Normaliza una ruta relativa dentro de `data_dir/photos` impidiendo salir del directorio.
    pub fn resolve_safe_path(&self, rel: &str) -> Result<PathBuf, PhotoError> {
        let base = self.data_dir.join("photos");
        if rel.contains("..") || rel.starts_with('/') || rel.starts_with('\\') {
            return Err(PhotoError::Forbidden(
                "Ruta de archivo inválida.".to_string(),
            ));
        }
        Ok(base.join(rel))
    }

    /// Recibe un bloque (o archivo completo) en una subida retomable y finaliza cuando se completa.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_chunk(
        &self,
        owner_id: i64,
        upload_id: Option<&str>,
        offset: u64,
        total_size: Option<u64>,
        filename: &str,
        content_type: Option<&str>,
        taken_at_hint: Option<i64>,
        bytes: &[u8],
    ) -> Result<UploadResult, PhotoError> {
        if bytes.is_empty() && total_size.unwrap_or(0) > 0 {
            return Err(PhotoError::BadRequest(
                "El archivo recibido está vacío.".to_string(),
            ));
        }

        let safe_name = sanitize_filename(filename);
        let uploads_dir = self.uploads_root();
        fs::create_dir_all(&uploads_dir).map_err(|_| {
            PhotoError::Internal("No pudimos preparar la carpeta de subidas.".to_string())
        })?;

        let uid = match upload_id {
            Some(id) => sanitize_upload_id(id)?,
            None => format!("single_{}_{}", owner_id, now_unix()),
        };
        let part_path = uploads_dir.join(format!("{owner_id}_{uid}.part"));

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(false)
            .open(&part_path)
            .map_err(|_| PhotoError::Internal("No pudimos guardar el bloque.".to_string()))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_| PhotoError::Internal("No pudimos posicionar la subida.".to_string()))?;
        file.write_all(bytes)
            .map_err(|_| PhotoError::Internal("No pudimos escribir el archivo.".to_string()))?;
        let current_len = file
            .metadata()
            .map(|m| m.len())
            .unwrap_or(offset + bytes.len() as u64);
        drop(file);

        let expected_total = total_size.unwrap_or(current_len);
        if current_len < expected_total {
            return Ok(UploadResult {
                complete: false,
                duplicate: false,
                received_bytes: current_len,
                photo: None,
            });
        }

        // Subida completa: calculamos blake3 en streaming para detectar duplicados.
        let hash = hash_file_streaming(&part_path)
            .map_err(|_| PhotoError::Internal("No pudimos verificar el archivo.".to_string()))?;

        if let Some(existing) = sqlx::query("SELECT * FROM photos WHERE owner_id = ? AND hash = ?")
            .bind(owner_id)
            .bind(&hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("Error al verificar duplicados.".to_string()))?
        {
            let _ = fs::remove_file(&part_path);
            return Ok(UploadResult {
                complete: true,
                duplicate: true,
                received_bytes: current_len,
                photo: Some(row_to_photo(&existing)),
            });
        }

        let (kind, mime_type) = detect_kind_and_mime(&safe_name, content_type);
        let taken_at = read_exif_taken_at(&part_path)
            .or(taken_at_hint)
            .unwrap_or_else(now_unix);
        let (year, month) = year_month_from_unix(taken_at);

        let rel_dir = format!("originals/{owner_id}/{year:04}/{month:02}");
        let final_dir = self.data_dir.join("photos").join(&rel_dir);
        fs::create_dir_all(&final_dir).map_err(|_| {
            PhotoError::Internal("No pudimos crear la carpeta del año/mes.".to_string())
        })?;

        let short_hash = &hash[..12];
        let rel_path = format!("{rel_dir}/{short_hash}_{safe_name}");
        let final_path = self.data_dir.join("photos").join(&rel_path);
        fs::rename(&part_path, &final_path)
            .or_else(|_| {
                fs::copy(&part_path, &final_path)?;
                fs::remove_file(&part_path)
            })
            .map_err(|_| {
                PhotoError::Internal("No pudimos guardar el archivo original.".to_string())
            })?;

        let res = sqlx::query(
            "INSERT INTO photos (owner_id, hash, path, filename, mime_type, kind, taken_at, size)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(owner_id)
        .bind(&hash)
        .bind(&rel_path)
        .bind(&safe_name)
        .bind(&mime_type)
        .bind(kind)
        .bind(taken_at)
        .bind(current_len as i64)
        .execute(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos registrar la foto.".to_string()))?;

        let photo_id = res.last_insert_rowid();
        let _ = self
            .jobs
            .enqueue("photo_thumb", &photo_id.to_string())
            .await;

        let row = sqlx::query("SELECT * FROM photos WHERE id = ?")
            .bind(photo_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos leer la foto guardada.".to_string()))?;

        Ok(UploadResult {
            complete: true,
            duplicate: false,
            received_bytes: current_len,
            photo: Some(row_to_photo(&row)),
        })
    }

    /// Genera miniaturas WebP de 256 px y 1024 px y color promedio en segundo plano.
    pub async fn generate_thumbnails(&self, photo_id: i64) -> Result<(), String> {
        let row = sqlx::query("SELECT id, path, kind FROM photos WHERE id = ?")
            .bind(photo_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        let Some(row) = row else {
            return Ok(());
        };

        let rel_path: String = row.get("path");
        let kind: String = row.get("kind");
        let src = self
            .resolve_safe_path(&rel_path)
            .map_err(|e| e.to_string())?;
        let thumbs_dir = self.thumbs_root();
        fs::create_dir_all(&thumbs_dir).map_err(|e| e.to_string())?;

        if kind == "video" {
            // Carga perezosa de ffmpeg: solo se ejecuta como subproceso cuando hace falta.
            let out_256 = thumbs_dir.join(format!("{photo_id}_256.webp"));
            let status = tokio::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-i",
                    &src.to_string_lossy(),
                    "-ss",
                    "00:00:01",
                    "-vframes",
                    "1",
                    "-vf",
                    "scale=256:-1",
                    &out_256.to_string_lossy(),
                ])
                .status()
                .await;
            if let Ok(st) = status
                && st.success()
            {
                return Ok(());
            }
            return Ok(());
        }

        let thumbs_dir_clone = thumbs_dir.clone();
        let (w, h, avg_color) =
            tokio::task::spawn_blocking(move || -> Result<(u32, u32, String), String> {
                let img = image::open(&src).map_err(|e| e.to_string())?;
                let w = img.width();
                let h = img.height();

                let tiny = img.resize_exact(1, 1, FilterType::Triangle).to_rgb8();
                let px = tiny.get_pixel(0, 0);
                let avg_color = format!("#{:02x}{:02x}{:02x}", px[0], px[1], px[2]);

                for size in [256_u32, 1024_u32] {
                    let resized = img.resize(size, size, FilterType::Triangle);
                    let out_path = thumbs_dir_clone.join(format!("{photo_id}_{size}.webp"));
                    resized
                        .save_with_format(&out_path, image::ImageFormat::WebP)
                        .map_err(|e| e.to_string())?;
                }
                Ok((w, h, avg_color))
            })
            .await
            .map_err(|e| e.to_string())??;

        let _ = sqlx::query("UPDATE photos SET width = ?, height = ?, avg_color = ? WHERE id = ?")
            .bind(i64::from(w))
            .bind(i64::from(h))
            .bind(avg_color)
            .bind(photo_id)
            .execute(&self.pool)
            .await;

        Ok(())
    }

    /// Verifica privacidad estricta: solo el dueño o alguien con álbum compartido puede ver la foto.
    pub async fn get_accessible_photo(
        &self,
        viewer_id: i64,
        photo_id: i64,
    ) -> Result<(Photo, PathBuf), PhotoError> {
        let row = sqlx::query("SELECT * FROM photos WHERE id = ?")
            .bind(photo_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos buscar la foto.".to_string()))?;

        let Some(row) = row else {
            return Err(PhotoError::NotFound("No encontramos esa foto.".to_string()));
        };

        let owner_id: i64 = row.get("owner_id");
        if owner_id != viewer_id {
            let shared: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM album_photos ap
                 JOIN album_shares s ON s.album_id = ap.album_id
                 WHERE ap.photo_id = ? AND s.user_id = ?",
            )
            .bind(photo_id)
            .bind(viewer_id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            if shared == 0 {
                return Err(PhotoError::Forbidden(
                    "No tenés permiso para ver esta foto.".to_string(),
                ));
            }
        }

        let rel_path: String = row.get("path");
        let abs = self.resolve_safe_path(&rel_path)?;
        Ok((row_to_photo(&row), abs))
    }

    /// Devuelve la ruta de una miniatura (`256` o `1024`) si ya existe.
    pub async fn get_thumb_path(
        &self,
        viewer_id: i64,
        photo_id: i64,
        size: u32,
    ) -> Result<PathBuf, PhotoError> {
        if !matches!(size, 256 | 1024) {
            return Err(PhotoError::BadRequest(
                "Tamaño de miniatura inválido (usá 256 o 1024).".to_string(),
            ));
        }
        let (_photo, orig_path) = self.get_accessible_photo(viewer_id, photo_id).await?;
        let thumb = self.thumbs_root().join(format!("{photo_id}_{size}.webp"));
        if thumb.is_file() {
            Ok(thumb)
        } else {
            Ok(orig_path)
        }
    }

    /// Lista fotos paginadas por cursor ordenadas por `taken_at DESC, id DESC`.
    #[allow(clippy::too_many_arguments)]
    pub async fn list_photos(
        &self,
        viewer_id: i64,
        cursor: Option<&str>,
        from: Option<i64>,
        to: Option<i64>,
        album_id: Option<i64>,
        trash: bool,
        limit: i64,
    ) -> Result<PhotoPage, PhotoError> {
        let limit = limit.clamp(1, 100);
        let (cursor_taken, cursor_id) = cursor
            .and_then(|c| c.split_once(':'))
            .and_then(|(t, i)| Some((t.parse::<i64>().ok()?, i.parse::<i64>().ok()?)))
            .unzip();

        let rows = if let Some(aid) = album_id {
            // Verifica que el usuario sea dueño o invitado del álbum.
            let allowed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM albums a
                 LEFT JOIN album_shares s ON s.album_id = a.id AND s.user_id = ?
                 WHERE a.id = ? AND (a.owner_id = ? OR s.user_id IS NOT NULL)",
            )
            .bind(viewer_id)
            .bind(aid)
            .bind(viewer_id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
            if allowed == 0 {
                return Err(PhotoError::Forbidden(
                    "No tenés permiso para ver este álbum.".to_string(),
                ));
            }

            sqlx::query(
                "SELECT p.* FROM photos p
                 JOIN album_photos ap ON ap.photo_id = p.id
                 WHERE ap.album_id = ? AND p.deleted_at IS NULL
                   AND (? IS NULL OR p.taken_at >= ?)
                   AND (? IS NULL OR p.taken_at <= ?)
                   AND (? IS NULL OR (p.taken_at < ? OR (p.taken_at = ? AND p.id < ?)))
                 ORDER BY p.taken_at DESC, p.id DESC
                 LIMIT ?",
            )
            .bind(aid)
            .bind(from)
            .bind(from)
            .bind(to)
            .bind(to)
            .bind(cursor_taken)
            .bind(cursor_taken)
            .bind(cursor_taken)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos listar el álbum.".to_string()))?
        } else if trash {
            sqlx::query(
                "SELECT * FROM photos
                 WHERE owner_id = ? AND deleted_at IS NOT NULL
                 ORDER BY deleted_at DESC, id DESC
                 LIMIT ?",
            )
            .bind(viewer_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos listar la papelera.".to_string()))?
        } else {
            sqlx::query(
                "SELECT * FROM photos
                 WHERE owner_id = ? AND deleted_at IS NULL
                   AND (? IS NULL OR taken_at >= ?)
                   AND (? IS NULL OR taken_at <= ?)
                   AND (? IS NULL OR (taken_at < ? OR (taken_at = ? AND id < ?)))
                 ORDER BY taken_at DESC, id DESC
                 LIMIT ?",
            )
            .bind(viewer_id)
            .bind(from)
            .bind(from)
            .bind(to)
            .bind(to)
            .bind(cursor_taken)
            .bind(cursor_taken)
            .bind(cursor_taken)
            .bind(cursor_id)
            .bind(limit + 1)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos cargar tus fotos.".to_string()))?
        };

        let mut items: Vec<Photo> = rows.iter().map(row_to_photo).collect();
        let next_cursor = if items.len() as i64 > limit {
            items.pop();
            items.last().map(|p| format!("{}:{}", p.taken_at, p.id))
        } else {
            None
        };

        Ok(PhotoPage { items, next_cursor })
    }

    /// Mueve una foto propia a la papelera (30 días de retención).
    pub async fn move_to_trash(&self, owner_id: i64, photo_id: i64) -> Result<(), PhotoError> {
        let res = sqlx::query(
            "UPDATE photos SET deleted_at = ? WHERE id = ? AND owner_id = ? AND deleted_at IS NULL",
        )
        .bind(now_unix())
        .bind(photo_id)
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos mover la foto a la papelera.".to_string()))?;

        if res.rows_affected() == 0 {
            return Err(PhotoError::NotFound("No encontramos esa foto.".to_string()));
        }
        Ok(())
    }

    /// Restaura una foto desde la papelera.
    pub async fn restore_photo(&self, owner_id: i64, photo_id: i64) -> Result<(), PhotoError> {
        let res = sqlx::query(
            "UPDATE photos SET deleted_at = NULL WHERE id = ? AND owner_id = ? AND deleted_at IS NOT NULL",
        )
        .bind(photo_id)
        .bind(owner_id)
        .execute(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos restaurar la foto.".to_string()))?;

        if res.rows_affected() == 0 {
            return Err(PhotoError::NotFound(
                "No encontramos esa foto en la papelera.".to_string(),
            ));
        }
        Ok(())
    }

    /// Elimina definitivamente fotos que llevan más de 30 días en la papelera.
    pub async fn purge_expired_trash(&self) -> Result<u64, PhotoError> {
        let cutoff = now_unix() - TRASH_RETENTION_SECS;
        let rows = sqlx::query(
            "SELECT id, path FROM photos WHERE deleted_at IS NOT NULL AND deleted_at < ?",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos revisar la papelera.".to_string()))?;

        let mut purged = 0;
        for row in rows {
            let id: i64 = row.get("id");
            let rel: String = row.get("path");
            if let Ok(path) = self.resolve_safe_path(&rel) {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_file(self.thumbs_root().join(format!("{id}_256.webp")));
            let _ = fs::remove_file(self.thumbs_root().join(format!("{id}_1024.webp")));
            let _ = sqlx::query("DELETE FROM photos WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await;
            purged += 1;
        }
        Ok(purged)
    }

    /// Lista álbumes propios y compartidos con `user_id`.
    pub async fn list_albums(&self, user_id: i64) -> Result<Vec<Album>, PhotoError> {
        let rows = sqlx::query(
            "SELECT a.id, a.owner_id, a.name, a.created_at,
                    CASE WHEN a.owner_id = ? THEN 0 ELSE 1 END AS is_shared,
                    CASE WHEN a.owner_id = ? THEN 1 ELSE COALESCE(s.can_edit, 0) END AS can_edit,
                    (SELECT COUNT(*) FROM album_photos ap JOIN photos p ON p.id = ap.photo_id
                     WHERE ap.album_id = a.id AND p.deleted_at IS NULL) AS photo_count
             FROM albums a
             LEFT JOIN album_shares s ON s.album_id = a.id AND s.user_id = ?
             WHERE a.owner_id = ? OR s.user_id IS NOT NULL
             ORDER BY a.created_at DESC",
        )
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos cargar los álbumes.".to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| Album {
                id: r.get("id"),
                owner_id: r.get("owner_id"),
                name: r.get("name"),
                created_at: r.get("created_at"),
                shared: r.get::<i64, _>("is_shared") != 0,
                can_edit: r.get::<i64, _>("can_edit") != 0,
                photo_count: r.get("photo_count"),
            })
            .collect())
    }

    pub async fn create_album(&self, owner_id: i64, name: &str) -> Result<Album, PhotoError> {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > 80 {
            return Err(PhotoError::BadRequest(
                "El nombre del álbum debe tener entre 1 y 80 caracteres.".to_string(),
            ));
        }
        let created_at = now_unix();
        let res = sqlx::query("INSERT INTO albums (owner_id, name, created_at) VALUES (?, ?, ?)")
            .bind(owner_id)
            .bind(trimmed)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos crear el álbum.".to_string()))?;

        Ok(Album {
            id: res.last_insert_rowid(),
            owner_id,
            name: trimmed.to_string(),
            created_at,
            shared: false,
            can_edit: true,
            photo_count: 0,
        })
    }

    pub async fn rename_album(
        &self,
        owner_id: i64,
        album_id: i64,
        name: &str,
    ) -> Result<(), PhotoError> {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > 80 {
            return Err(PhotoError::BadRequest(
                "El nombre del álbum debe tener entre 1 y 80 caracteres.".to_string(),
            ));
        }
        let res = sqlx::query("UPDATE albums SET name = ? WHERE id = ? AND owner_id = ?")
            .bind(trimmed)
            .bind(album_id)
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos renombrar el álbum.".to_string()))?;
        if res.rows_affected() == 0 {
            return Err(PhotoError::NotFound(
                "No encontramos ese álbum.".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn delete_album(&self, owner_id: i64, album_id: i64) -> Result<(), PhotoError> {
        let res = sqlx::query("DELETE FROM albums WHERE id = ? AND owner_id = ?")
            .bind(album_id)
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(|_| PhotoError::Internal("No pudimos eliminar el álbum.".to_string()))?;
        if res.rows_affected() == 0 {
            return Err(PhotoError::NotFound(
                "No encontramos ese álbum.".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn share_album(
        &self,
        owner_id: i64,
        album_id: i64,
        target_user_id: i64,
        can_edit: bool,
    ) -> Result<(), PhotoError> {
        let owns: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM albums WHERE id = ? AND owner_id = ?")
                .bind(album_id)
                .bind(owner_id)
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
        if owns == 0 {
            return Err(PhotoError::Forbidden(
                "Solo quien creó el álbum puede compartirlo.".to_string(),
            ));
        }

        sqlx::query(
            "INSERT INTO album_shares (album_id, user_id, can_edit) VALUES (?, ?, ?)
             ON CONFLICT(album_id, user_id) DO UPDATE SET can_edit = excluded.can_edit",
        )
        .bind(album_id)
        .bind(target_user_id)
        .bind(i64::from(can_edit))
        .execute(&self.pool)
        .await
        .map_err(|_| PhotoError::Internal("No pudimos compartir el álbum.".to_string()))?;
        Ok(())
    }

    pub async fn add_photo_to_album(
        &self,
        user_id: i64,
        album_id: i64,
        photo_id: i64,
    ) -> Result<(), PhotoError> {
        let allowed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM albums a
             LEFT JOIN album_shares s ON s.album_id = a.id AND s.user_id = ?
             WHERE a.id = ? AND (a.owner_id = ? OR s.can_edit = 1)",
        )
        .bind(user_id)
        .bind(album_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);
        if allowed == 0 {
            return Err(PhotoError::Forbidden(
                "No tenés permiso para editar este álbum.".to_string(),
            ));
        }
        let _ = self.get_accessible_photo(user_id, photo_id).await?;

        sqlx::query("INSERT OR IGNORE INTO album_photos (album_id, photo_id) VALUES (?, ?)")
            .bind(album_id)
            .bind(photo_id)
            .execute(&self.pool)
            .await
            .map_err(|_| {
                PhotoError::Internal("No pudimos agregar la foto al álbum.".to_string())
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthService, Role};

    fn tiny_png_bytes() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([59, 105, 57]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        cursor.into_inner()
    }

    #[tokio::test]
    async fn chunked_upload_deduplicates_generates_thumbs_and_enforces_privacy() {
        let db = Database::connect_in_memory().await.unwrap();
        let auth = AuthService::new(&db);
        let admin = auth
            .setup_admin("Franco", "contrasena-admin-2026", "ua")
            .await
            .unwrap();
        let member = auth
            .create_user("Sofi", "clave-miembro-2026", Role::Member)
            .await
            .unwrap();

        let temp_dir = std::env::temp_dir().join(format!("roost-photos-test-{}", now_unix()));
        let jobs = JobQueue::new(&db);
        let photos = PhotoService::new(&db, temp_dir.clone(), jobs);

        let png = tiny_png_bytes();
        let half = png.len() / 2;

        // Subida en 2 bloques retomables por parte de Sofi (member).
        let part1 = photos
            .upload_chunk(
                member.id,
                Some("up1"),
                0,
                Some(png.len() as u64),
                "vacaciones.png",
                Some("image/png"),
                Some(1_700_000_000),
                &png[..half],
            )
            .await
            .unwrap();
        assert!(!part1.complete);

        let part2 = photos
            .upload_chunk(
                member.id,
                Some("up1"),
                half as u64,
                Some(png.len() as u64),
                "vacaciones.png",
                Some("image/png"),
                Some(1_700_000_000),
                &png[half..],
            )
            .await
            .unwrap();
        assert!(part2.complete);
        assert!(!part2.duplicate);
        let photo = part2.photo.unwrap();

        // Subir la misma foto otra vez se detecta como duplicada por blake3.
        let dup = photos
            .upload_chunk(
                member.id,
                Some("up2"),
                0,
                Some(png.len() as u64),
                "copia.png",
                Some("image/png"),
                None,
                &png,
            )
            .await
            .unwrap();
        assert!(dup.duplicate);
        assert_eq!(dup.photo.unwrap().id, photo.id);

        // Miniaturas WebP de 256 y 1024 px.
        photos.generate_thumbnails(photo.id).await.unwrap();
        assert!(
            temp_dir
                .join(format!("photos/thumbs/{}_256.webp", photo.id))
                .is_file()
        );
        assert!(
            temp_dir
                .join(format!("photos/thumbs/{}_1024.webp", photo.id))
                .is_file()
        );

        // Privacidad estricta: ni siquiera el admin puede ver la foto privada de Sofi.
        assert!(matches!(
            photos.get_accessible_photo(admin.user.id, photo.id).await,
            Err(PhotoError::Forbidden(_))
        ));

        // Si Sofi comparte un álbum con el admin, recién ahí puede verla.
        let album = photos.create_album(member.id, "Viaje").await.unwrap();
        photos
            .add_photo_to_album(member.id, album.id, photo.id)
            .await
            .unwrap();
        photos
            .share_album(member.id, album.id, admin.user.id, false)
            .await
            .unwrap();
        assert!(
            photos
                .get_accessible_photo(admin.user.id, photo.id)
                .await
                .is_ok()
        );

        // Papelera y restauración.
        photos.move_to_trash(member.id, photo.id).await.unwrap();
        assert!(
            photos
                .list_photos(member.id, None, None, None, None, false, 20)
                .await
                .unwrap()
                .items
                .is_empty()
        );
        photos.restore_photo(member.id, photo.id).await.unwrap();
        assert_eq!(
            photos
                .list_photos(member.id, None, None, None, None, false, 20)
                .await
                .unwrap()
                .items
                .len(),
            1
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
