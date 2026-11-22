use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::Serialize;
use tokio::sync::broadcast;

use crate::docker::{ContainerState, DockerError, DockerRuntime, InstallSpec};
use crate::metrics::SystemSnapshot;

use super::manifest::{
    AppCategory, AppManifest, check_ram_warning, extract_primary_image, load_catalog,
};

/// Eventos en tiempo real enviados por SSE (`/api/events`) solo cuando pasa algo.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoostEvent {
    AppInstallProgress {
        app_id: String,
        phase: String,
        downloaded_mb: f64,
        total_mb: f64,
        percent: u8,
    },
    AppInstallFinished {
        app_id: String,
        status: ContainerState,
    },
    AppInstallFailed {
        app_id: String,
        message: String,
    },
    AppStateChanged {
        app_id: String,
        status: String,
    },
    SystemMetrics(SystemSnapshot),
}

/// App instalada vista por la interfaz.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub category: AppCategory,
    pub version: String,
    pub web_port: u16,
    pub status: ContainerState,
}

/// Respuesta al iniciar la instalación de una app.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstallResponse {
    pub app_id: String,
    pub status: String,
    pub ram_warning: Option<String>,
}

/// Errores de negocio de la API de apps, siempre con mensajes simples en español.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    NotFound(String),
    BadRequest(String),
    RuntimeUnavailable(String),
    Internal(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(msg)
            | Self::BadRequest(msg)
            | Self::RuntimeUnavailable(msg)
            | Self::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AppError {}

fn map_docker_error(app_name: &str, action: &str, err: DockerError) -> AppError {
    match err {
        DockerError::NotConnected(_) => AppError::RuntimeUnavailable(
            "El motor de apps no está disponible en este momento. Verificá que el sistema esté encendido y probá de nuevo.".to_string(),
        ),
        DockerError::NotFound(_) => AppError::NotFound(format!(
            "No encontramos {app_name} entre tus apps instaladas."
        )),
        DockerError::InvalidId(_) => AppError::BadRequest(
            "El nombre de la app no es válido.".to_string(),
        ),
        DockerError::OperationFailed(_) => AppError::Internal(format!(
            "No pudimos {action} {app_name}. Probá de nuevo en unos segundos."
        )),
    }
}

/// Proveedor de memoria libre en MB (en M5 se conecta con `sysinfo`).
pub type RamProvider = Arc<dyn Fn() -> u64 + Send + Sync>;

/// Servicio principal del catálogo y ciclo de vida de apps.
pub struct AppService {
    catalog: RwLock<Vec<AppManifest>>,
    images: RwLock<HashMap<String, String>>,
    icons: HashMap<String, String>,
    docker: Arc<dyn DockerRuntime>,
    events: broadcast::Sender<RoostEvent>,
    available_ram_mb: RamProvider,
    data_dir: PathBuf,
}

impl AppService {
    /// Crea el servicio cargando el catálogo oficial desde `apps_dir`.
    pub fn from_dir(
        apps_dir: &Path,
        data_dir: PathBuf,
        docker: Arc<dyn DockerRuntime>,
        available_ram_mb: RamProvider,
    ) -> Result<Self, super::CatalogError> {
        let catalog = load_catalog(apps_dir)?;
        let mut images = HashMap::with_capacity(catalog.len());
        let mut icons = HashMap::with_capacity(catalog.len());
        for app in &catalog {
            let compose_path = apps_dir.join(&app.id).join("compose.yaml");
            if let Ok(raw) = fs::read_to_string(&compose_path)
                && let Some(image) = extract_primary_image(&raw)
            {
                images.insert(app.id.clone(), image);
            }
            let icon_path = apps_dir.join(&app.id).join(&app.icon);
            if let Ok(svg) = fs::read_to_string(&icon_path) {
                icons.insert(app.id.clone(), svg);
            }
        }
        Ok(Self::new_with_icons(
            catalog,
            images,
            icons,
            data_dir,
            docker,
            available_ram_mb,
        ))
    }

    /// Crea el servicio a partir de un catálogo ya cargado en memoria.
    pub fn new(
        catalog: Vec<AppManifest>,
        images: HashMap<String, String>,
        data_dir: PathBuf,
        docker: Arc<dyn DockerRuntime>,
        available_ram_mb: RamProvider,
    ) -> Self {
        Self::new_with_icons(
            catalog,
            images,
            HashMap::new(),
            data_dir,
            docker,
            available_ram_mb,
        )
    }

    fn new_with_icons(
        catalog: Vec<AppManifest>,
        images: HashMap<String, String>,
        icons: HashMap<String, String>,
        data_dir: PathBuf,
        docker: Arc<dyn DockerRuntime>,
        available_ram_mb: RamProvider,
    ) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            catalog: RwLock::new(catalog),
            images: RwLock::new(images),
            icons,
            docker,
            events,
            available_ram_mb,
            data_dir,
        }
    }

    /// Devuelve el SVG del ícono de una app del catálogo.
    pub fn icon_svg(&self, app_id: &str) -> Option<&str> {
        self.icons.get(app_id).map(String::as_str)
    }

    /// Devuelve el catálogo completo disponible en la Tienda (oficiales + sincronizadas).
    pub fn store(&self) -> Vec<AppManifest> {
        self.catalog.read().expect("catalog lock poisoned").clone()
    }

    /// Sincroniza apps reales desde la API pública de LinuxServer.io sin sobreescribir
    /// las apps oficiales curadas en español.
    pub async fn sync_remote_catalog(&self) -> Result<usize, AppError> {
        let remote = super::remote_catalog::fetch_live_catalog()
            .await
            .map_err(AppError::RuntimeUnavailable)?;

        let mut catalog = self.catalog.write().expect("catalog lock poisoned");
        let mut images = self.images.write().expect("images lock poisoned");

        let mut added = 0;
        for (manifest, image_ref) in remote {
            if catalog.iter().any(|existing| existing.id == manifest.id) {
                continue;
            }
            images.insert(manifest.id.clone(), image_ref);
            catalog.push(manifest);
            added += 1;
        }
        Ok(added)
    }

    /// Suscribe un cliente SSE a los eventos en vivo de Roost.
    pub fn subscribe(&self) -> broadcast::Receiver<RoostEvent> {
        self.events.subscribe()
    }

    /// Devuelve el emisor de eventos para publicar métricas en vivo bajo demanda.
    pub fn event_sender(&self) -> broadcast::Sender<RoostEvent> {
        self.events.clone()
    }

    fn find_manifest(&self, app_id: &str) -> Result<AppManifest, AppError> {
        self.catalog
            .read()
            .expect("catalog lock poisoned")
            .iter()
            .find(|app| app.id == app_id)
            .cloned()
            .ok_or_else(|| {
                AppError::NotFound("No encontramos esa app en la Tienda de Roost.".to_string())
            })
    }

    fn container_name(app_id: &str) -> String {
        format!("roost-{app_id}")
    }

    /// Lista las apps del catálogo que actualmente están instaladas.
    pub async fn installed_apps(&self) -> Result<Vec<InstalledApp>, AppError> {
        let containers = self
            .docker
            .list_containers(true)
            .await
            .map_err(|err| map_docker_error("tus apps", "consultar", err))?;

        let catalog = self.catalog.read().expect("catalog lock poisoned").clone();
        let mut installed = Vec::new();
        for container in containers {
            let Some(app_id) = container.app_id else {
                continue;
            };
            if let Some(manifest) = catalog.iter().find(|a| a.id == app_id) {
                installed.push(InstalledApp {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    tagline: manifest.tagline.clone(),
                    category: manifest.category,
                    version: manifest.version.clone(),
                    web_port: manifest.web_port,
                    status: container.state,
                });
            }
        }

        installed.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(installed)
    }

    /// Inicia la instalación de una app en segundo plano y devuelve el aviso de RAM si hace falta.
    pub fn install(&self, app_id: &str) -> Result<InstallResponse, AppError> {
        let manifest = self.find_manifest(app_id)?;
        let image = self
            .images
            .read()
            .expect("images lock poisoned")
            .get(&manifest.id)
            .cloned()
            .unwrap_or_else(|| format!("{}:{}", manifest.id, manifest.version));

        let ram_warning = check_ram_warning(&manifest, (self.available_ram_mb)());

        let spec = InstallSpec {
            app_id: manifest.id.clone(),
            container_name: Self::container_name(&manifest.id),
            image,
            web_port: manifest.web_port,
            host_data_dir: self
                .data_dir
                .join("apps")
                .join(&manifest.id)
                .to_string_lossy()
                .into_owned(),
        };

        let docker = Arc::clone(&self.docker);
        let events = self.events.clone();
        let app_id_owned = manifest.id.clone();
        let app_name = manifest.name.clone();

        let _ = events.send(RoostEvent::AppInstallProgress {
            app_id: app_id_owned.clone(),
            phase: "starting".to_string(),
            downloaded_mb: 0.0,
            total_mb: 0.0,
            percent: 0,
        });

        tokio::spawn(async move {
            let progress_events = events.clone();
            let progress_app_id = app_id_owned.clone();
            let on_progress = Arc::new(move |p: crate::docker::InstallProgress| {
                let downloaded_mb = (p.downloaded_bytes as f64) / (1024.0 * 1024.0);
                let total_mb = (p.total_bytes as f64) / (1024.0 * 1024.0);
                let percent = p
                    .downloaded_bytes
                    .saturating_mul(100)
                    .checked_div(p.total_bytes)
                    .unwrap_or(0)
                    .min(100) as u8;
                let _ = progress_events.send(RoostEvent::AppInstallProgress {
                    app_id: progress_app_id.clone(),
                    phase: p.phase,
                    downloaded_mb: (downloaded_mb * 10.0).round() / 10.0,
                    total_mb: (total_mb * 10.0).round() / 10.0,
                    percent,
                });
            });

            match docker.install_app(spec, on_progress).await {
                Ok(info) => {
                    let _ = events.send(RoostEvent::AppInstallFinished {
                        app_id: app_id_owned,
                        status: info.state,
                    });
                }
                Err(err) => {
                    let message = map_docker_error(&app_name, "instalar", err).to_string();
                    let _ = events.send(RoostEvent::AppInstallFailed {
                        app_id: app_id_owned,
                        message,
                    });
                }
            }
        });

        Ok(InstallResponse {
            app_id: manifest.id,
            status: "installing".to_string(),
            ram_warning,
        })
    }

    /// Inicia una app instalada.
    pub async fn start(&self, app_id: &str) -> Result<(), AppError> {
        let manifest = self.find_manifest(app_id)?;
        let name = Self::container_name(&manifest.id);
        self.docker
            .start_container(&name)
            .await
            .map_err(|err| map_docker_error(&manifest.name, "iniciar", err))?;

        let _ = self.events.send(RoostEvent::AppStateChanged {
            app_id: manifest.id.clone(),
            status: "running".to_string(),
        });
        Ok(())
    }

    /// Detiene una app instalada.
    pub async fn stop(&self, app_id: &str) -> Result<(), AppError> {
        let manifest = self.find_manifest(app_id)?;
        let name = Self::container_name(&manifest.id);
        self.docker
            .stop_container(&name)
            .await
            .map_err(|err| map_docker_error(&manifest.name, "detener", err))?;

        let _ = self.events.send(RoostEvent::AppStateChanged {
            app_id: manifest.id.clone(),
            status: "stopped".to_string(),
        });
        Ok(())
    }

    /// Reinicia una app instalada.
    pub async fn restart(&self, app_id: &str) -> Result<(), AppError> {
        let manifest = self.find_manifest(app_id)?;
        let name = Self::container_name(&manifest.id);
        self.docker
            .restart_container(&name)
            .await
            .map_err(|err| map_docker_error(&manifest.name, "reiniciar", err))?;

        let _ = self.events.send(RoostEvent::AppStateChanged {
            app_id: manifest.id.clone(),
            status: "running".to_string(),
        });
        Ok(())
    }

    /// Desinstala una app instalada (requiere confirmación previa en la API).
    pub async fn uninstall(&self, app_id: &str) -> Result<(), AppError> {
        let manifest = self.find_manifest(app_id)?;
        let name = Self::container_name(&manifest.id);
        self.docker
            .remove_container(&name)
            .await
            .map_err(|err| map_docker_error(&manifest.name, "desinstalar", err))?;

        let _ = self.events.send(RoostEvent::AppStateChanged {
            app_id: manifest.id.clone(),
            status: "uninstalled".to_string(),
        });
        Ok(())
    }
}
