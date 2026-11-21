use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::query_parameters::{
    InspectContainerOptionsBuilder, ListContainersOptionsBuilder, StartContainerOptionsBuilder,
    StopContainerOptionsBuilder,
};

use super::{BoxFuture, ContainerInfo, ContainerState, DockerError, DockerRuntime};

/// Implementación real de [`DockerRuntime`] usando `bollard` sobre el socket local.
#[derive(Clone, Debug)]
pub struct BollardDocker {
    client: Docker,
}

impl BollardDocker {
    /// Conecta con el socket local de Docker (`/var/run/docker.sock` en Linux/macOS).
    pub fn connect_local() -> Result<Self, DockerError> {
        let client = Docker::connect_with_local_defaults()
            .map_err(|err| DockerError::NotConnected(err.to_string()))?;
        Ok(Self { client })
    }

    /// Conecta con un socket Unix específico (útil para entornos personalizados).
    #[cfg(unix)]
    pub fn connect_unix(path: &str) -> Result<Self, DockerError> {
        let client = Docker::connect_with_unix(path, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|err| DockerError::NotConnected(err.to_string()))?;
        Ok(Self { client })
    }
}

fn map_bollard_error(id: Option<&str>, err: BollardError) -> DockerError {
    match err {
        BollardError::DockerResponseServerError {
            status_code: 404, ..
        } => DockerError::NotFound(id.unwrap_or("desconocida").to_string()),
        BollardError::IOError { err } => DockerError::NotConnected(err.to_string()),
        BollardError::HyperResponseError { err } => DockerError::NotConnected(err.to_string()),
        other => DockerError::OperationFailed(other.to_string()),
    }
}

impl DockerRuntime for BollardDocker {
    fn list_containers(
        &self,
        include_stopped: bool,
    ) -> BoxFuture<'_, Result<Vec<ContainerInfo>, DockerError>> {
        Box::pin(async move {
            let options = ListContainersOptionsBuilder::default()
                .all(include_stopped)
                .build();

            let containers = self
                .client
                .list_containers(Some(options))
                .await
                .map_err(|err| map_bollard_error(None, err))?;

            let mut result = Vec::with_capacity(containers.len());
            for item in containers {
                let id = item.id.unwrap_or_default();
                let name = item
                    .names
                    .and_then(|mut names| names.drain(..).next())
                    .map(|n| n.trim_start_matches('/').to_string())
                    .unwrap_or_else(|| id.clone());
                let image = item.image.unwrap_or_default();
                let state = item
                    .state
                    .map(|s| ContainerState::from_docker_str(s.as_ref()))
                    .unwrap_or(ContainerState::Unknown("empty".to_string()));
                let status_text = item.status.unwrap_or_default();
                let app_id = item
                    .labels
                    .and_then(|mut labels| labels.remove("roost.app"));

                result.push(ContainerInfo {
                    id,
                    name,
                    image,
                    state,
                    status_text,
                    app_id,
                });
            }

            Ok(result)
        })
    }

    fn container_status(&self, id: &str) -> BoxFuture<'_, Result<ContainerState, DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = super::validate_container_id(&id)?;
            let options = InspectContainerOptionsBuilder::default().build();
            let details = self
                .client
                .inspect_container(id, Some(options))
                .await
                .map_err(|err| map_bollard_error(Some(id), err))?;

            let state = details
                .state
                .and_then(|s| s.status)
                .map(|status| ContainerState::from_docker_str(status.as_ref()))
                .unwrap_or(ContainerState::Unknown("empty".to_string()));

            Ok(state)
        })
    }

    fn start_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = super::validate_container_id(&id)?;
            let options = StartContainerOptionsBuilder::default().build();
            match self.client.start_container(id, Some(options)).await {
                Ok(()) => Ok(()),
                Err(BollardError::DockerResponseServerError {
                    status_code: 304, ..
                }) => Ok(()),
                Err(err) => Err(map_bollard_error(Some(id), err)),
            }
        })
    }

    fn stop_container(&self, id: &str) -> BoxFuture<'_, Result<(), DockerError>> {
        let id = id.to_string();
        Box::pin(async move {
            let id = super::validate_container_id(&id)?;
            let options = StopContainerOptionsBuilder::default().t(10).build();
            match self.client.stop_container(id, Some(options)).await {
                Ok(()) => Ok(()),
                Err(BollardError::DockerResponseServerError {
                    status_code: 304, ..
                }) => Ok(()),
                Err(err) => Err(map_bollard_error(Some(id), err)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prueba manual contra un daemon de Docker real.
    ///
    /// Ejecutar con: `cargo test real_docker_smoke -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "requiere un daemon de Docker local en ejecución"]
    async fn real_docker_smoke() {
        let docker = BollardDocker::connect_local().expect("no se pudo conectar a Docker local");
        let containers = docker
            .list_containers(true)
            .await
            .expect("no se pudieron listar los contenedores");

        println!("Contenedores encontrados: {}", containers.len());
        for c in &containers {
            let status = docker
                .container_status(&c.id)
                .await
                .expect("no se pudo consultar el estado");
            println!("- {} ({}) => {}", c.name, c.image, status);
        }
    }
}
