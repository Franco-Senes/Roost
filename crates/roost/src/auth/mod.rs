use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::db::Database;

/// Longitud mínima de contraseña (`AGENTS.md` §12).
pub const MIN_PASSWORD_LEN: usize = 10;
/// Duración de sesión: 30 días en segundos (`AGENTS.md` §12).
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Límite de intentos fallidos de inicio de sesión: 5 en 15 minutos (`AGENTS.md` §12).
pub const MAX_LOGIN_ATTEMPTS: i64 = 5;
pub const LOGIN_WINDOW_SECS: i64 = 15 * 60;

/// Rol de un usuario de la familia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

/// Representación pública de un usuario (nunca incluye `password_hash`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub role: Role,
    pub suspended: bool,
    pub created_at: i64,
}

/// Sesión verificada junto con el token CSRF asociado.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub csrf_token: String,
    pub expires_at: i64,
    pub user: User,
}

/// Errores de autenticación y permisos en español simple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    TooManyRequests(String),
    Conflict(String),
    NotFound(String),
    Internal(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::TooManyRequests(m)
            | Self::Conflict(m)
            | Self::NotFound(m)
            | Self::Internal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AuthError {}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Genera un token aleatorio criptográfico de 256 bits en hexadecimal (64 caracteres).
pub fn generate_token_256() -> Result<String, AuthError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| {
        AuthError::Internal("No pudimos generar un código seguro de sesión.".to_string())
    })?;
    Ok(hex::encode(bytes))
}

/// Comparación en tiempo constante para validar tokens CSRF sin filtrar información.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn argon2_hasher() -> Argon2<'static> {
    // Parámetros balanceados para Raspberry Pi / máquinas con 2-4 GB de RAM.
    let params = Params::new(19 * 1024, 2, 1, None).unwrap_or_default();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    validate_password(password)?;
    let mut salt_bytes = [0u8; 16];
    getrandom::getrandom(&mut salt_bytes)
        .map_err(|_| AuthError::Internal("No pudimos proteger la contraseña.".to_string()))?;
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|_| AuthError::Internal("No pudimos proteger la contraseña.".to_string()))?;
    argon2_hasher()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AuthError::Internal("No pudimos proteger la contraseña.".to_string()))
}

fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    argon2_hasher()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn validate_username(name: &str) -> Result<String, AuthError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(AuthError::BadRequest(
            "El nombre de usuario debe tener entre 1 y 64 caracteres.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AuthError::BadRequest(format!(
            "La contraseña debe tener al menos {MIN_PASSWORD_LEN} caracteres."
        )));
    }
    Ok(())
}

/// Servicio de usuarios, sesiones, roles y protección contra fuerza bruta.
#[derive(Clone, Debug)]
pub struct AuthService {
    pool: SqlitePool,
}

impl AuthService {
    pub fn new(db: &Database) -> Self {
        Self {
            pool: db.pool().clone(),
        }
    }

    /// Devuelve `true` si todavía no existe ningún usuario y falta crear al administrador.
    pub async fn needs_setup(&self) -> Result<bool, AuthError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(|_| {
                AuthError::Internal("No pudimos consultar el estado inicial.".to_string())
            })?;
        Ok(count == 0)
    }

    /// Crea el primer usuario administrador en el arranque inicial e inicia su sesión.
    pub async fn setup_admin(
        &self,
        name: &str,
        password: &str,
        user_agent: &str,
    ) -> Result<SessionInfo, AuthError> {
        if !self.needs_setup().await? {
            return Err(AuthError::Conflict(
                "El administrador ya fue creado. Iniciá sesión con tu cuenta.".to_string(),
            ));
        }

        let user = self.create_user(name, password, Role::Admin).await?;
        self.create_session(user, user_agent).await
    }

    /// Inicia sesión aplicando límite de 5 intentos fallidos cada 15 minutos por IP y usuario.
    pub async fn login(
        &self,
        ip: &str,
        name: &str,
        password: &str,
        user_agent: &str,
    ) -> Result<SessionInfo, AuthError> {
        let username = name.trim();
        let now = now_unix();
        let cutoff = now - LOGIN_WINDOW_SECS;

        // Limpiamos intentos viejos para mantener la tabla chica.
        let _ = sqlx::query("DELETE FROM login_attempts WHERE attempted_at < ?")
            .bind(cutoff)
            .execute(&self.pool)
            .await;

        let ip_fails: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts WHERE ip = ? AND attempted_at >= ?",
        )
        .bind(ip)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let user_fails: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM login_attempts WHERE username = ? AND attempted_at >= ?",
        )
        .bind(username)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        if ip_fails >= MAX_LOGIN_ATTEMPTS || user_fails >= MAX_LOGIN_ATTEMPTS {
            return Err(AuthError::TooManyRequests(
                "Demasiados intentos fallidos. Esperá 15 minutos y probá de nuevo.".to_string(),
            ));
        }

        let row = sqlx::query(
            "SELECT id, name, password_hash, role, suspended, created_at FROM users WHERE name = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AuthError::Internal("No pudimos verificar tus datos.".to_string()))?;

        let Some(row) = row else {
            self.record_failed_attempt(ip, username, now).await;
            return Err(AuthError::Unauthorized(
                "Usuario o contraseña incorrectos.".to_string(),
            ));
        };

        let password_hash: String = row.get("password_hash");
        if !verify_password(password, &password_hash) {
            self.record_failed_attempt(ip, username, now).await;
            return Err(AuthError::Unauthorized(
                "Usuario o contraseña incorrectos.".to_string(),
            ));
        }

        let suspended: i64 = row.get("suspended");
        if suspended != 0 {
            return Err(AuthError::Forbidden(
                "Tu cuenta está suspendida. Pedile al administrador que la vuelva a activar."
                    .to_string(),
            ));
        }

        let role_str: String = row.get("role");
        let user = User {
            id: row.get("id"),
            name: row.get("name"),
            role: Role::parse(&role_str).unwrap_or(Role::Member),
            suspended: false,
            created_at: row.get("created_at"),
        };

        // Limpiamos intentos fallidos tras un acceso correcto.
        let _ = sqlx::query("DELETE FROM login_attempts WHERE ip = ? OR username = ?")
            .bind(ip)
            .bind(username)
            .execute(&self.pool)
            .await;

        self.create_session(user, user_agent).await
    }

    async fn record_failed_attempt(&self, ip: &str, username: &str, now: i64) {
        let _ =
            sqlx::query("INSERT INTO login_attempts (ip, username, attempted_at) VALUES (?, ?, ?)")
                .bind(ip)
                .bind(username)
                .bind(now)
                .execute(&self.pool)
                .await;
    }

    async fn create_session(&self, user: User, user_agent: &str) -> Result<SessionInfo, AuthError> {
        let session_id = generate_token_256()?;
        let csrf_token = generate_token_256()?;
        let expires_at = now_unix() + SESSION_TTL_SECS;

        sqlx::query(
            "INSERT INTO sessions (id, user_id, csrf_token, expires_at, user_agent) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&session_id)
        .bind(user.id)
        .bind(&csrf_token)
        .bind(expires_at)
        .bind(user_agent)
        .execute(&self.pool)
        .await
        .map_err(|_| AuthError::Internal("No pudimos guardar la sesión.".to_string()))?;

        Ok(SessionInfo {
            session_id,
            csrf_token,
            expires_at,
            user,
        })
    }

    /// Verifica una cookie de sesión activa y que el usuario no esté suspendido.
    pub async fn verify_session(&self, session_id: &str) -> Result<SessionInfo, AuthError> {
        let now = now_unix();
        let row = sqlx::query(
            "SELECT s.id AS sid, s.csrf_token, s.expires_at,
                    u.id AS uid, u.name, u.role, u.suspended, u.created_at
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| AuthError::Internal("No pudimos validar tu sesión.".to_string()))?;

        let Some(row) = row else {
            return Err(AuthError::Unauthorized(
                "Tenés que iniciar sesión para continuar.".to_string(),
            ));
        };

        let expires_at: i64 = row.get("expires_at");
        if expires_at <= now {
            let _ = self.logout(session_id).await;
            return Err(AuthError::Unauthorized(
                "Tu sesión venció. Volvé a iniciar sesión.".to_string(),
            ));
        }

        let suspended: i64 = row.get("suspended");
        if suspended != 0 {
            return Err(AuthError::Forbidden(
                "Tu cuenta está suspendida. Pedile al administrador que la vuelva a activar."
                    .to_string(),
            ));
        }

        let role_str: String = row.get("role");
        Ok(SessionInfo {
            session_id: row.get("sid"),
            csrf_token: row.get("csrf_token"),
            expires_at,
            user: User {
                id: row.get("uid"),
                name: row.get("name"),
                role: Role::parse(&role_str).unwrap_or(Role::Member),
                suspended: false,
                created_at: row.get("created_at"),
            },
        })
    }

    /// Cierra una sesión específica.
    pub async fn logout(&self, session_id: &str) -> Result<(), AuthError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|_| AuthError::Internal("No pudimos cerrar la sesión.".to_string()))?;
        Ok(())
    }

    /// Lista todos los usuarios registrados.
    pub async fn list_users(&self) -> Result<Vec<User>, AuthError> {
        let rows =
            sqlx::query("SELECT id, name, role, suspended, created_at FROM users ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(|_| {
                    AuthError::Internal("No pudimos cargar la lista de usuarios.".to_string())
                })?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let role_str: String = r.get("role");
                let suspended: i64 = r.get("suspended");
                User {
                    id: r.get("id"),
                    name: r.get("name"),
                    role: Role::parse(&role_str).unwrap_or(Role::Member),
                    suspended: suspended != 0,
                    created_at: r.get("created_at"),
                }
            })
            .collect())
    }

    /// Crea un nuevo usuario con contraseña protegida por `argon2id`.
    pub async fn create_user(
        &self,
        name: &str,
        password: &str,
        role: Role,
    ) -> Result<User, AuthError> {
        let clean_name = validate_username(name)?;
        let password_hash = hash_password(password)?;
        let created_at = now_unix();

        let res = sqlx::query(
            "INSERT INTO users (name, password_hash, role, suspended, created_at) VALUES (?, ?, ?, 0, ?)",
        )
        .bind(&clean_name)
        .bind(&password_hash)
        .bind(role.as_str())
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|err| {
            if err.to_string().contains("UNIQUE") {
                AuthError::Conflict("Ya existe una persona con ese nombre.".to_string())
            } else {
                AuthError::Internal("No pudimos crear el usuario.".to_string())
            }
        })?;

        Ok(User {
            id: res.last_insert_rowid(),
            name: clean_name,
            role,
            suspended: false,
            created_at,
        })
    }

    /// Actualiza el rol o estado de suspensión de un usuario.
    pub async fn update_user(
        &self,
        actor_id: i64,
        target_id: i64,
        role: Option<Role>,
        suspended: Option<bool>,
    ) -> Result<User, AuthError> {
        if actor_id == target_id && (suspended == Some(true) || role == Some(Role::Member)) {
            return Err(AuthError::BadRequest(
                "No podés suspenderte ni quitarte el rol de administrador a vos mismo.".to_string(),
            ));
        }

        let existing =
            sqlx::query("SELECT id, name, role, suspended, created_at FROM users WHERE id = ?")
                .bind(target_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| AuthError::Internal("No pudimos consultar el usuario.".to_string()))?;

        let Some(row) = existing else {
            return Err(AuthError::NotFound(
                "No encontramos ese usuario.".to_string(),
            ));
        };

        let current_role = Role::parse(&row.get::<String, _>("role")).unwrap_or(Role::Member);
        let current_suspended = row.get::<i64, _>("suspended") != 0;

        let next_role = role.unwrap_or(current_role);
        let next_suspended = suspended.unwrap_or(current_suspended);

        sqlx::query("UPDATE users SET role = ?, suspended = ? WHERE id = ?")
            .bind(next_role.as_str())
            .bind(if next_suspended { 1_i64 } else { 0_i64 })
            .bind(target_id)
            .execute(&self.pool)
            .await
            .map_err(|_| AuthError::Internal("No pudimos actualizar el usuario.".to_string()))?;

        if next_suspended {
            // Revocamos de inmediato todas las sesiones activas del usuario suspendido.
            let _ = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(target_id)
                .execute(&self.pool)
                .await;
        }

        Ok(User {
            id: target_id,
            name: row.get("name"),
            role: next_role,
            suspended: next_suspended,
            created_at: row.get("created_at"),
        })
    }

    /// Elimina un usuario (no permite eliminarse a sí mismo).
    pub async fn delete_user(&self, actor_id: i64, target_id: i64) -> Result<(), AuthError> {
        if actor_id == target_id {
            return Err(AuthError::BadRequest(
                "No podés eliminar tu propia cuenta.".to_string(),
            ));
        }

        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(target_id)
            .execute(&self.pool)
            .await
            .map_err(|_| AuthError::Internal("No pudimos eliminar el usuario.".to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AuthError::NotFound(
                "No encontramos ese usuario.".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_auth() -> AuthService {
        let db = Database::connect_in_memory().await.unwrap();
        AuthService::new(&db)
    }

    #[tokio::test]
    async fn setup_requires_strong_password_and_runs_once() {
        let auth = test_auth().await;
        assert!(auth.needs_setup().await.unwrap());

        let err = auth
            .setup_admin("Franco", "corta", "test-agent")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::BadRequest(_)));

        let session = auth
            .setup_admin("Franco", "contrasena-segura-123", "test-agent")
            .await
            .unwrap();
        assert_eq!(session.user.role, Role::Admin);
        assert_eq!(session.session_id.len(), 64);
        assert_eq!(session.csrf_token.len(), 64);
        assert!(!auth.needs_setup().await.unwrap());

        let dup = auth
            .setup_admin("Otro", "otra-contrasena-123", "test-agent")
            .await
            .unwrap_err();
        assert!(matches!(dup, AuthError::Conflict(_)));
    }

    #[tokio::test]
    async fn rate_limits_after_five_failed_attempts() {
        let auth = test_auth().await;
        auth.setup_admin("Franco", "contrasena-segura-123", "ua")
            .await
            .unwrap();

        for _ in 0..5 {
            let err = auth
                .login("192.168.1.50", "Franco", "incorrecta-123", "ua")
                .await
                .unwrap_err();
            assert!(matches!(err, AuthError::Unauthorized(_)));
        }

        let blocked = auth
            .login("192.168.1.50", "Franco", "contrasena-segura-123", "ua")
            .await
            .unwrap_err();
        assert!(matches!(blocked, AuthError::TooManyRequests(_)));
    }

    #[tokio::test]
    async fn suspending_user_revokes_session_and_blocks_login() {
        let auth = test_auth().await;
        let admin = auth
            .setup_admin("Franco", "contrasena-segura-123", "ua")
            .await
            .unwrap();
        let member = auth
            .create_user("Sofi", "clave-familiar-2026", Role::Member)
            .await
            .unwrap();
        let member_session = auth
            .login("192.168.1.20", "Sofi", "clave-familiar-2026", "ua")
            .await
            .unwrap();

        auth.update_user(admin.user.id, member.id, None, Some(true))
            .await
            .unwrap();

        assert!(matches!(
            auth.verify_session(&member_session.session_id).await,
            Err(AuthError::Unauthorized(_))
        ));
        assert!(matches!(
            auth.login("192.168.1.20", "Sofi", "clave-familiar-2026", "ua")
                .await,
            Err(AuthError::Forbidden(_))
        ));
    }
}
