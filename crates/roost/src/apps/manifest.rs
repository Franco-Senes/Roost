use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Categorías permitidas en la Tienda de Roost (`AGENTS.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppCategory {
    Fotos,
    #[serde(rename = "Películas")]
    Peliculas,
    Archivos,
    Red,
    Casa,
    Seguridad,
}

impl fmt::Display for AppCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fotos => write!(f, "Fotos"),
            Self::Peliculas => write!(f, "Películas"),
            Self::Archivos => write!(f, "Archivos"),
            Self::Red => write!(f, "Red"),
            Self::Casa => write!(f, "Casa"),
            Self::Seguridad => write!(f, "Seguridad"),
        }
    }
}

/// Metadatos de una app definidos en `apps/<id>/app.toml` o sincronizados desde el catálogo remoto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppManifest {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub description: String,
    pub category: AppCategory,
    pub version: String,
    pub web_port: u16,
    pub ram_recommended_mb: u64,
    pub icon: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    pub license: String,
    pub upstream: String,
}

/// Errores al cargar o validar el catálogo de apps.
#[derive(Debug)]
pub enum CatalogError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    Validation {
        app_id: String,
        reason: String,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "no se pudo leer '{}': {source}", path.display())
            }
            Self::Parse { path, message } => {
                write!(f, "formato inválido en '{}': {message}", path.display())
            }
            Self::Validation { app_id, reason } => {
                write!(f, "la app '{app_id}' no cumple el formato: {reason}")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

const FORBIDDEN_JARGON: &[&str] = &["docker", "contenedor", "volumen", "puerto", "imagen"];

impl AppManifest {
    /// Parsea y valida un archivo `app.toml` desde texto.
    pub fn from_toml_str(raw: &str, source_path: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let manifest: Self = toml::from_str(raw).map_err(|err| CatalogError::Parse {
            path: source_path.as_ref().to_path_buf(),
            message: err.to_string(),
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Valida todas las reglas del catálogo (`AGENTS.md` §6).
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.id.starts_with('-')
            || self.id.ends_with('-')
        {
            return Err(self.err("id debe usar solo minúsculas, números y guiones"));
        }

        if self.name.trim().is_empty() {
            return Err(self.err("name no puede estar vacío"));
        }
        if self.tagline.trim().is_empty() {
            return Err(self.err("tagline no puede estar vacío"));
        }
        if self.description.trim().is_empty() {
            return Err(self.err("description no puede estar vacío"));
        }

        for field_value in [&self.tagline, &self.description] {
            let lower = field_value.to_ascii_lowercase();
            for word in FORBIDDEN_JARGON {
                if lower.contains(word) {
                    return Err(self.err(format!(
                        "los textos para el usuario no deben incluir la palabra técnica '{word}'"
                    )));
                }
            }
        }

        let version_trimmed = self.version.trim();
        if version_trimmed.is_empty() || version_trimmed.eq_ignore_ascii_case("latest") {
            return Err(self.err("version debe estar fijada y nunca puede ser 'latest'"));
        }

        if self.web_port == 0 {
            return Err(self.err("web_port debe ser mayor que 0"));
        }
        if self.ram_recommended_mb == 0 {
            return Err(self.err("ram_recommended_mb debe ser mayor que 0"));
        }

        if self.icon.is_empty()
            || self.icon.contains('/')
            || self.icon.contains('\\')
            || self.icon.contains("..")
        {
            return Err(self.err("icon debe ser un nombre de archivo relativo sin subcarpetas"));
        }

        if self.license.trim().is_empty() {
            return Err(self.err("license no puede estar vacío"));
        }
        if !self.upstream.starts_with("https://") {
            return Err(self.err("upstream debe comenzar con https://"));
        }

        Ok(())
    }

    fn err(&self, reason: impl Into<String>) -> CatalogError {
        CatalogError::Validation {
            app_id: if self.id.is_empty() {
                "<vacío>".to_string()
            } else {
                self.id.clone()
            },
            reason: reason.into(),
        }
    }
}

/// Extrae la primera imagen fijada declarada en `compose.yaml`.
pub fn extract_primary_image(compose_raw: &str) -> Option<String> {
    for raw_line in compose_raw.lines() {
        let line = raw_line.trim();
        if let Some(rest) = line.strip_prefix("image:") {
            let image = rest.trim().trim_matches('"').trim_matches('\'');
            if !image.is_empty() {
                return Some(image.to_string());
            }
        }
    }
    None
}

/// Valida que `compose.yaml` cumpla las reglas del catálogo (versiones fijadas,
/// volúmenes dentro de `apps/<id>/` y sin `privileged: true`).
pub fn validate_compose(app_id: &str, compose_raw: &str) -> Result<(), CatalogError> {
    let mut has_image = false;

    for raw_line in compose_raw.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some(rest) = line.strip_prefix("image:") {
            has_image = true;
            let image = rest.trim().trim_matches('"').trim_matches('\'');
            if image.ends_with(":latest") || !image.contains(':') {
                return Err(CatalogError::Validation {
                    app_id: app_id.to_string(),
                    reason: format!("imagen sin versión fijada en compose.yaml: '{image}'"),
                });
            }
        }

        if line
            .replace(' ', "")
            .eq_ignore_ascii_case("privileged:true")
        {
            return Err(CatalogError::Validation {
                app_id: app_id.to_string(),
                reason: "compose.yaml no puede usar privileged: true".to_string(),
            });
        }

        if line.starts_with("- ") && line.contains(":/") {
            let expected_prefix = format!("/apps/{app_id}/");
            if !line.contains(&expected_prefix) {
                return Err(CatalogError::Validation {
                    app_id: app_id.to_string(),
                    reason: format!(
                        "todo volumen debe vivir dentro de apps/{app_id}/ (encontrado: '{line}')"
                    ),
                });
            }
        }
    }

    if !has_image {
        return Err(CatalogError::Validation {
            app_id: app_id.to_string(),
            reason: "compose.yaml debe declarar al menos una imagen".to_string(),
        });
    }

    Ok(())
}

/// Carga y valida todas las apps de un directorio `apps/`.
pub fn load_catalog(apps_dir: &Path) -> Result<Vec<AppManifest>, CatalogError> {
    let entries = fs::read_dir(apps_dir).map_err(|source| CatalogError::Io {
        path: apps_dir.to_path_buf(),
        source,
    })?;

    let mut manifests = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| CatalogError::Io {
            path: apps_dir.to_path_buf(),
            source,
        })?;
        let app_dir = entry.path();
        if !app_dir.is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = app_dir.join("app.toml");
        let raw_toml = fs::read_to_string(&manifest_path).map_err(|source| CatalogError::Io {
            path: manifest_path.clone(),
            source,
        })?;

        let manifest = AppManifest::from_toml_str(&raw_toml, &manifest_path)?;
        if manifest.id != dir_name {
            return Err(CatalogError::Validation {
                app_id: manifest.id,
                reason: format!("el id debe coincidir con el nombre de la carpeta '{dir_name}'"),
            });
        }

        let icon_path = app_dir.join(&manifest.icon);
        if !icon_path.is_file() {
            return Err(CatalogError::Validation {
                app_id: manifest.id,
                reason: format!("falta el archivo de ícono '{}'", manifest.icon),
            });
        }

        let compose_path = app_dir.join("compose.yaml");
        let compose_raw = fs::read_to_string(&compose_path).map_err(|source| CatalogError::Io {
            path: compose_path,
            source,
        })?;
        validate_compose(&manifest.id, &compose_raw)?;

        manifests.push(manifest);
    }

    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(manifests)
}

/// Compara la RAM recomendada por una app con la RAM libre del equipo.
///
/// Devuelve `Some(aviso)` en español simple si la memoria libre es menor a la recomendada.
pub fn check_ram_warning(manifest: &AppManifest, available_ram_mb: u64) -> Option<String> {
    if available_ram_mb < manifest.ram_recommended_mb {
        Some(format!(
            "{} recomienda {} MB de memoria libre, pero ahora hay {} MB disponibles. Puede andar lento o cerrarse si la instalás.",
            manifest.name, manifest.ram_recommended_mb, available_ram_mb
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_toml() -> &'static str {
        r#"
id = "jellyfin"
name = "Jellyfin"
tagline = "Tus películas en cualquier pantalla"
description = "Mirá tus películas y series en la tele, la compu o el celular."
category = "Películas"
version = "10.10.7"
web_port = 8096
ram_recommended_mb = 1024
icon = "icon.svg"
license = "GPL-2.0-only"
upstream = "https://jellyfin.org"
"#
    }

    #[test]
    fn parses_valid_manifest() {
        let app = AppManifest::from_toml_str(valid_toml(), "app.toml").unwrap();
        assert_eq!(app.id, "jellyfin");
        assert_eq!(app.category, AppCategory::Peliculas);
        assert_eq!(app.web_port, 8096);
    }

    #[test]
    fn rejects_latest_version() {
        let bad = valid_toml().replace("10.10.7", "latest");
        let err = AppManifest::from_toml_str(&bad, "app.toml").unwrap_err();
        assert!(matches!(err, CatalogError::Validation { .. }));
    }

    #[test]
    fn rejects_technical_jargon_in_user_copy() {
        let bad = valid_toml().replace(
            "Tus películas en cualquier pantalla",
            "Contenedor Docker de películas",
        );
        let err = AppManifest::from_toml_str(&bad, "app.toml").unwrap_err();
        assert!(matches!(err, CatalogError::Validation { .. }));
    }

    #[test]
    fn rejects_compose_with_latest_or_privileged_or_bad_volume() {
        assert!(validate_compose("demo", "services:\n  a:\n    image: demo:latest\n").is_err());
        assert!(
            validate_compose(
                "demo",
                "services:\n  a:\n    image: demo:1.0\n    privileged: true\n"
            )
            .is_err()
        );
        assert!(
            validate_compose(
                "demo",
                "services:\n  a:\n    image: demo:1.0\n    volumes:\n      - /etc:/etc\n"
            )
            .is_err()
        );
    }

    #[test]
    fn ram_warning_triggers_only_when_insufficient() {
        let app = AppManifest::from_toml_str(valid_toml(), "app.toml").unwrap();
        assert!(check_ram_warning(&app, 1024).is_none());
        assert!(check_ram_warning(&app, 2048).is_none());
        let warning = check_ram_warning(&app, 512).unwrap();
        assert!(warning.contains("Jellyfin"));
        assert!(warning.contains("1024 MB"));
        assert!(warning.contains("512 MB"));
    }

    #[test]
    fn repository_catalog_contains_the_five_valid_apps() {
        let apps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let apps = load_catalog(&apps_dir).expect("el catálogo oficial debe ser válido");

        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "adguard-home",
                "home-assistant",
                "jellyfin",
                "nextcloud",
                "vaultwarden"
            ]
        );
    }
}
