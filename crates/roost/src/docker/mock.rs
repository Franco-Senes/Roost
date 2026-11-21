use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{
    BoxFuture, ContainerInfo, ContainerState, DockerError, DockerRuntime, InstallProgress,
    InstallSpec, ProgressCallback, validate_container_id,
};

#[derive(Debug, Default)]
struct MockState {
    containers: BTreeMap<String, ContainerInfo>,
    disconnected: bool,
}

/// Simulador en memoria de [`DockerRuntime`] para tests y modo demo.
#[derive(Debug, Default)]
pub struct MockDocker {
    state: Mutex<MockState>,
}

impl MockDocker {
    /// Crea un simulador vacío y conectado.
    pub fn new() -> Self {
        Self::default()
    }

    /// Crea un simulador precargado con las instancias indicadas.
    pub fn with_containers(containers: impl IntoIterator<Item = ContainerInfo>) -> Self {
        let map = containers
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect::<BTreeMap<_, _>>();
        Self {
            state: Mutex::new(MockState {
                containers: map,
                disconnected: false,
            }),
        }
    }

    /// Agrega o reemplaza una instancia en el simulador.
    pub fn add_container(&self, container: ContainerInfo) {
        let mut guard = self.state.lock().expect("mock lock poisoned");
        guard.containers.insert(container.id.clone(), container);
    }

    /// Simula que el daemon de Docker no responde.
    pub fn set_disconnected(&self, disconnected: bool) {
        let mut guard = self.state.lock().expect("mock lock poisoned");
        guard.disconnected = disconnected;
    }

    fn find_key(containers: &BTreeMap<String, ContainerInfo>, id_or_name: &str) -> Option<String> {
        if containers.contains_key(id_or_name) {
            return Some(id_or_name.to_string());
        }
        containers
            .iter()
            .find(|(_, info)| info.name == id_or_name)
            .map(|(key, _)| key.clone())
    }
}

impl DockerRuntime for MockDocker {
    fn list_containers(
        &self,
        include_stopped: bool,
    ) -> BoxFuture<'_, Result<Vec<ContainerInfo>, DockerError>> {
        Box::pin(async move {
            let guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let items = guard
                .containers
                .values()
                .filter(|c| include_stopped || c.state == ContainerState::Running)
                .cloned()
                .collect();

            Ok(items)
        })
    }

    fn container_status(&self, id: &str) -> BoxFuture<'_, Result<ContainerState, DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = validate_container_id(&id)?;
            let guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let key = Self::find_key(&guard.containers, id)
                .ok_or_else(|| DockerError::NotFound(id.to_string()))?;
            Ok(guard.containers[&key].state.clone())
        })
    }

    fn start_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = validate_container_id(&id)?;
            let mut guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let key = Self::find_key(&guard.containers, id)
                .ok_or_else(|| DockerError::NotFound(id.to_string()))?;
            if let Some(container) = guard.containers.get_mut(&key) {
                container.state = ContainerState::Running;
                container.status_text = "Up just now".to_string();
            }
            Ok(())
        })
    }

    fn stop_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = validate_container_id(&id)?;
            let mut guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let key = Self::find_key(&guard.containers, id)
                .ok_or_else(|| DockerError::NotFound(id.to_string()))?;
            if let Some(container) = guard.containers.get_mut(&key) {
                container.state = ContainerState::Stopped;
                container.status_text = "Exited (0)".to_string();
            }
            Ok(())
        })
    }

    fn restart_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = validate_container_id(&id)?;
            let mut guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let key = Self::find_key(&guard.containers, id)
                .ok_or_else(|| DockerError::NotFound(id.to_string()))?;
            if let Some(container) = guard.containers.get_mut(&key) {
                container.state = ContainerState::Running;
                container.status_text = "Restarted just now".to_string();
            }
            Ok(())
        })
    }

    fn remove_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = validate_container_id(&id)?;
            let mut guard = self.state.lock().expect("mock lock poisoned");
            if guard.disconnected {
                return Err(DockerError::NotConnected(
                    "simulador desconectado".to_string(),
                ));
            }

            let key = Self::find_key(&guard.containers, id)
                .ok_or_else(|| DockerError::NotFound(id.to_string()))?;
            guard.containers.remove(&key);
            Ok(())
        })
    }

    fn install_app(
        &self,
        spec: InstallSpec,
        on_progress: ProgressCallback,
    ) -> BoxFuture<'_, Result<ContainerInfo, DockerError>> {
        Box::pin(async move {
            let container_name = validate_container_id(&spec.container_name)?.to_string();
            {
                let guard = self.state.lock().expect("mock lock poisoned");
                if guard.disconnected {
                    return Err(DockerError::NotConnected(
                        "simulador desconectado".to_string(),
                    ));
                }
            }

            let total_bytes: u64 = 100 * 1024 * 1024;
            for step in [25_u64, 50, 75, 100] {
                on_progress(InstallProgress {
                    phase: "downloading".to_string(),
                    downloaded_bytes: total_bytes * step / 100,
                    total_bytes,
                });
                tokio::task::yield_now().await;
            }

            on_progress(InstallProgress {
                phase: "starting".to_string(),
                downloaded_bytes: total_bytes,
                total_bytes,
            });

            let info = ContainerInfo {
                id: format!("mock-{}", spec.app_id),
                name: container_name,
                image: spec.image,
                state: ContainerState::Running,
                status_text: "Up just now".to_string(),
                app_id: Some(spec.app_id),
            };

            let mut guard = self.state.lock().expect("mock lock poisoned");
            guard.containers.insert(info.id.clone(), info.clone());
            Ok(info)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn sample_container(id: &str, name: &str, state: ContainerState) -> ContainerInfo {
        ContainerInfo {
            id: id.to_string(),
            name: name.to_string(),
            image: format!("ghcr.io/roost/{name}:1.0"),
            state,
            status_text: "sample".to_string(),
            app_id: Some(name.to_string()),
        }
    }

    #[tokio::test]
    async fn list_filters_stopped_containers_when_requested() {
        let docker = MockDocker::with_containers([
            sample_container("c1", "jellyfin", ContainerState::Running),
            sample_container("c2", "nextcloud", ContainerState::Stopped),
        ]);

        let running = docker.list_containers(false).await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].name, "jellyfin");

        let all = docker.list_containers(true).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn start_and_stop_update_status_and_are_idempotent() {
        let docker = MockDocker::with_containers([sample_container(
            "c1",
            "jellyfin",
            ContainerState::Stopped,
        )]);

        assert_eq!(
            docker.container_status("c1").await.unwrap(),
            ContainerState::Stopped
        );

        docker.start_container("jellyfin").await.unwrap();
        assert_eq!(
            docker.container_status("c1").await.unwrap(),
            ContainerState::Running
        );

        docker.start_container("c1").await.unwrap();
        assert_eq!(
            docker.container_status("c1").await.unwrap(),
            ContainerState::Running
        );

        docker.stop_container("c1").await.unwrap();
        assert_eq!(
            docker.container_status("jellyfin").await.unwrap(),
            ContainerState::Stopped
        );

        docker.stop_container("c1").await.unwrap();
        assert_eq!(
            docker.container_status("c1").await.unwrap(),
            ContainerState::Stopped
        );
    }

    #[tokio::test]
    async fn restart_and_remove_work_on_existing_containers() {
        let docker = MockDocker::with_containers([sample_container(
            "c1",
            "jellyfin",
            ContainerState::Stopped,
        )]);

        docker.restart_container("c1").await.unwrap();
        assert_eq!(
            docker.container_status("c1").await.unwrap(),
            ContainerState::Running
        );

        docker.remove_container("c1").await.unwrap();
        assert_eq!(
            docker.container_status("c1").await,
            Err(DockerError::NotFound("c1".to_string()))
        );
    }

    #[tokio::test]
    async fn install_emits_progress_and_registers_running_container() {
        let docker = MockDocker::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);

        let info = docker
            .install_app(
                InstallSpec {
                    app_id: "jellyfin".to_string(),
                    container_name: "roost-jellyfin".to_string(),
                    image: "jellyfin/jellyfin:10.10.7".to_string(),
                    web_port: 8096,
                    host_data_dir: "/tmp/roost/apps/jellyfin".to_string(),
                },
                Arc::new(move |p| sink.lock().unwrap().push(p)),
            )
            .await
            .unwrap();

        assert_eq!(info.state, ContainerState::Running);
        assert_eq!(info.app_id.as_deref(), Some("jellyfin"));
        let recorded = events.lock().unwrap();
        assert!(recorded.len() >= 4);
        assert_eq!(recorded.last().unwrap().phase, "starting");
    }

    #[tokio::test]
    async fn missing_container_returns_not_found() {
        let docker = MockDocker::new();

        assert_eq!(
            docker.container_status("nope").await,
            Err(DockerError::NotFound("nope".to_string()))
        );
        assert_eq!(
            docker.start_container("nope").await,
            Err(DockerError::NotFound("nope".to_string()))
        );
        assert_eq!(
            docker.stop_container("nope").await,
            Err(DockerError::NotFound("nope".to_string()))
        );
    }

    #[tokio::test]
    async fn invalid_id_is_rejected_before_lookup() {
        let docker = MockDocker::new();

        assert!(matches!(
            docker.container_status("../etc").await,
            Err(DockerError::InvalidId(_))
        ));
        assert!(matches!(
            docker.start_container("").await,
            Err(DockerError::InvalidId(_))
        ));
        assert!(matches!(
            docker.stop_container("bad name").await,
            Err(DockerError::InvalidId(_))
        ));
    }

    #[tokio::test]
    async fn disconnected_mock_returns_not_connected() {
        let docker = MockDocker::with_containers([sample_container(
            "c1",
            "vaultwarden",
            ContainerState::Running,
        )]);
        docker.set_disconnected(true);

        assert!(matches!(
            docker.list_containers(true).await,
            Err(DockerError::NotConnected(_))
        ));
        assert!(matches!(
            docker.container_status("c1").await,
            Err(DockerError::NotConnected(_))
        ));
        assert!(matches!(
            docker.start_container("c1").await,
            Err(DockerError::NotConnected(_))
        ));
        assert!(matches!(
            docker.stop_container("c1").await,
            Err(DockerError::NotConnected(_))
        ));
    }

    #[tokio::test]
    async fn trait_is_dyn_compatible() {
        let runtime: Arc<dyn DockerRuntime> =
            Arc::new(MockDocker::with_containers([sample_container(
                "c1",
                "adguard",
                ContainerState::Running,
            )]));

        let list = runtime.list_containers(false).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app_id.as_deref(), Some("adguard"));
    }
}
