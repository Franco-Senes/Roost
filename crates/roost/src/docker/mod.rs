pub mod bollard_client;
pub mod mock;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use bollard_client::BollardDocker;
pub use mock::MockDocker;

/// Futuro encajonado y `Send` usado por [`DockerRuntime`] para mantener el trait
/// compatible con objetos trait (`dyn DockerRuntime`) sin sumar dependencias.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Callback liviano para informar el avance real de una instalación (MB descargados).
pub type ProgressCallback = Arc<dyn Fn(InstallProgress) + Send + Sync>;

/// Avance de descarga/instalación de una app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallProgress {
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

/// Especificación mínima para instalar una app del catálogo en Docker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSpec {
    pub app_id: String,
    pub container_name: String,
    pub image: String,
    pub web_port: u16,
    pub host_data_dir: String,
}

/// Estado simplificado de una instancia gestionada por Docker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    Running,
    Stopped,
    Paused,
    Restarting,
    Creating,
    Dead,
    Unknown(String),
}

impl ContainerState {
    /// Convierte un texto de estado de la API de Docker en [`ContainerState`].
    pub fn from_docker_str(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "running" => Self::Running,
            "exited" | "stopped" | "created" => Self::Stopped,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "removing" => Self::Creating,
            "dead" => Self::Dead,
            other => Self::Unknown(other.to_string()),
        }
    }
}

impl fmt::Display for ContainerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Paused => write!(f, "paused"),
            Self::Restarting => write!(f, "restarting"),
            Self::Creating => write!(f, "creating"),
            Self::Dead => write!(f, "dead"),
            Self::Unknown(raw) => write!(f, "{raw}"),
        }
    }
}

/// Resumen de un servicio/contenedor devuelto por [`DockerRuntime::list_containers`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
    pub status_text: String,
    /// Identificador de la app de Roost si está presente en la etiqueta `roost.app`.
    pub app_id: Option<String>,
}

/// Errores del módulo `docker`.
///
/// Los detalles técnicos se mantienen encapsulados en este módulo; las capas
/// superiores traducen estos errores a mensajes simples en español para la familia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerError {
    NotConnected(String),
    NotFound(String),
    InvalidId(String),
    OperationFailed(String),
}

impl fmt::Display for DockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected(msg) => write!(f, "no hay conexión con el motor de apps: {msg}"),
            Self::NotFound(id) => write!(f, "no se encontró la instancia '{id}'"),
            Self::InvalidId(id) => write!(f, "identificador inválido: '{id}'"),
            Self::OperationFailed(msg) => write!(f, "falló la operación: {msg}"),
        }
    }
}

impl std::error::Error for DockerError {}

/// Valida que un identificador o nombre sea seguro antes de enviarlo al socket.
pub fn validate_container_id(id: &str) -> Result<&str, DockerError> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || trimmed != id
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(DockerError::InvalidId(id.to_string()));
    }
    Ok(trimmed)
}

/// Interfaz única para interactuar con Docker.
///
/// Toda operación de Roost sobre Docker pasa por este trait para permitir:
/// - pruebas unitarias y de integración rápidas con [`MockDocker`] sin Docker real,
/// - modo demo del emulador (M8),
/// - aislamiento estricto del socket de Docker dentro de `crate::docker`.
pub trait DockerRuntime: Send + Sync {
    /// Lista las instancias existentes. Si `include_stopped` es `false`, solo devuelve las activas.
    fn list_containers(
        &self,
        include_stopped: bool,
    ) -> BoxFuture<'_, Result<Vec<ContainerInfo>, DockerError>>;

    /// Consulta el estado actual de una instancia por su id o nombre.
    fn container_status(&self, id: &str) -> BoxFuture<'_, Result<ContainerState, DockerError>>;

    /// Inicia una instancia detenida. Si ya está en ejecución, finaliza sin error.
    fn start_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>>;

    /// Detiene una instancia en ejecución. Si ya está detenida, finaliza sin error.
    fn stop_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>>;

    /// Reinicia una instancia existente.
    fn restart_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>>;

    /// Elimina una instancia instalada (deteniéndola si hace falta).
    fn remove_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>>;

    /// Descarga, crea e inicia una app del catálogo reportando el progreso real.
    fn install_app(
        &self,
        spec: InstallSpec,
        on_progress: ProgressCallback,
    ) -> BoxFuture<'_, Result<ContainerInfo, DockerError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_state_parses_known_values() {
        assert_eq!(
            ContainerState::from_docker_str("running"),
            ContainerState::Running
        );
        assert_eq!(
            ContainerState::from_docker_str("exited"),
            ContainerState::Stopped
        );
        assert_eq!(
            ContainerState::from_docker_str("paused"),
            ContainerState::Paused
        );
        assert_eq!(
            ContainerState::from_docker_str("restarting"),
            ContainerState::Restarting
        );
        assert_eq!(
            ContainerState::from_docker_str("dead"),
            ContainerState::Dead
        );
        assert_eq!(
            ContainerState::from_docker_str("custom"),
            ContainerState::Unknown("custom".to_string())
        );
    }

    #[test]
    fn validate_container_id_accepts_safe_ids() {
        assert_eq!(validate_container_id("jellyfin"), Ok("jellyfin"));
        assert_eq!(
            validate_container_id("roost_adguard-1.0"),
            Ok("roost_adguard-1.0")
        );
    }

    #[test]
    fn validate_container_id_rejects_unsafe_ids() {
        for bad in ["", "   ", "bad id", "../escape", "a/b", "id\nnewline"] {
            assert!(
                matches!(validate_container_id(bad), Err(DockerError::InvalidId(_))),
                "debería rechazar '{bad}'"
            );
        }
    }
}
