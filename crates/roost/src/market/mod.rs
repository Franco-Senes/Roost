use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::db::Database;

/// Herramienta declarativa expuesta por una habilidad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillTool {
    pub name: String,
    pub description: String,
}

/// Definición de una habilidad en `skills/<id>/skill.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub tools: Vec<SkillTool>,
}

/// Vista de una habilidad en el Marketplace junto con su estado y permisos en español.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSkillView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub permissions: Vec<String>,
    pub permissions_es: Vec<String>,
    pub tools: Vec<SkillTool>,
    pub installed: bool,
    pub enabled: bool,
}

/// Valida si un permiso pertenece a la lista cerrada de Roost (`AGENTS.md` §10).
pub fn is_valid_permission(perm: &str) -> bool {
    if matches!(perm, "fotos:leer" | "apps:instalar" | "sistema:leer") {
        return true;
    }
    if let Some(rest) = perm.strip_prefix("app:")
        && let Some(app_id) = rest.strip_suffix(":api")
    {
        return !app_id.is_empty()
            && app_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    }
    false
}

/// Traduce un permiso técnico a una explicación clara en español para el admin.
pub fn explain_permission_es(perm: &str) -> String {
    match perm {
        "fotos:leer" => "Buscar en tus fotos y álbumes propios".to_string(),
        "apps:instalar" => "Proponer instalar apps desde la Tienda".to_string(),
        "sistema:leer" => "Ver el estado del equipo (procesador, memoria y disco)".to_string(),
        other => {
            if let Some(rest) = other.strip_prefix("app:")
                && let Some(app_id) = rest.strip_suffix(":api")
            {
                return format!("Consultar la app {app_id} instalada en tu casa");
            }
            other.to_string()
        }
    }
}

impl SkillManifest {
    pub fn from_toml_str(raw: &str) -> Result<Self, String> {
        let skill: Self = toml::from_str(raw).map_err(|e| format!("skill.toml inválido: {e}"))?;
        skill.validate()?;
        Ok(skill)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("id de habilidad inválido".to_string());
        }
        if self.name.trim().is_empty() || self.description.trim().is_empty() {
            return Err("name y description son obligatorios".to_string());
        }
        if self.version.trim().is_empty() {
            return Err("version es obligatoria".to_string());
        }
        for perm in &self.permissions {
            if !is_valid_permission(perm) {
                return Err(format!("permiso fuera de la lista permitida: '{perm}'"));
            }
        }
        if self.tools.is_empty() {
            return Err("la habilidad debe declarar al menos una herramienta".to_string());
        }
        Ok(())
    }
}

pub fn load_skills_dir(dir: &Path) -> Result<Vec<SkillManifest>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut list = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("skill.toml");
        let raw = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("no se pudo leer {}: {e}", manifest_path.display()))?;
        list.push(SkillManifest::from_toml_str(&raw)?);
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Servicio del Marketplace de habilidades declarativas con control estricto de permisos.
#[derive(Clone)]
pub struct MarketService {
    pool: SqlitePool,
    catalog: Vec<SkillManifest>,
}

impl MarketService {
    pub fn from_dir(db: &Database, skills_dir: &Path) -> Self {
        let catalog = load_skills_dir(skills_dir).unwrap_or_default();
        Self {
            pool: db.pool().clone(),
            catalog,
        }
    }

    pub async fn list_skills(&self) -> Result<Vec<MarketSkillView>, String> {
        let rows = sqlx::query("SELECT skill_id, enabled FROM skills_installed")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| "No pudimos consultar las habilidades instaladas.".to_string())?;

        Ok(self
            .catalog
            .iter()
            .map(|s| {
                let installed_row = rows.iter().find(|r| r.get::<String, _>("skill_id") == s.id);
                let installed = installed_row.is_some();
                let enabled = installed_row
                    .map(|r| r.get::<i64, _>("enabled") != 0)
                    .unwrap_or(false);
                MarketSkillView {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    description: s.description.clone(),
                    version: s.version.clone(),
                    permissions_es: s
                        .permissions
                        .iter()
                        .map(|p| explain_permission_es(p))
                        .collect(),
                    permissions: s.permissions.clone(),
                    tools: s.tools.clone(),
                    installed,
                    enabled,
                }
            })
            .collect())
    }

    /// Instala una habilidad verificando que el administrador aprobó todos los permisos declarados.
    pub async fn install_skill(
        &self,
        skill_id: &str,
        approved_permissions: &[String],
    ) -> Result<MarketSkillView, String> {
        let skill = self
            .catalog
            .iter()
            .find(|s| s.id == skill_id)
            .ok_or_else(|| "No encontramos esa habilidad en el Marketplace.".to_string())?;

        for required in &skill.permissions {
            if !approved_permissions.contains(required) {
                return Err(format!(
                    "Falta aprobar el permiso '{}' para instalar {}.",
                    explain_permission_es(required),
                    skill.name
                ));
            }
        }
        // Nunca concede más permisos que los declarados en skill.toml.
        let granted_json = serde_json::to_string(&skill.permissions).map_err(|e| e.to_string())?;

        sqlx::query(
            "INSERT INTO skills_installed (skill_id, version, permissions_granted, enabled, installed_at)
             VALUES (?, ?, ?, 1, ?)
             ON CONFLICT(skill_id) DO UPDATE SET
               version = excluded.version,
               permissions_granted = excluded.permissions_granted,
               enabled = 1",
        )
        .bind(&skill.id)
        .bind(&skill.version)
        .bind(&granted_json)
        .bind(now_unix())
        .execute(&self.pool)
        .await
        .map_err(|_| "No pudimos instalar la habilidad.".to_string())?;

        let all = self.list_skills().await?;
        all.into_iter()
            .find(|s| s.id == skill_id)
            .ok_or_else(|| "Error al leer la habilidad instalada.".to_string())
    }

    /// Verifica si una habilidad instalada tiene concedido un permiso específico.
    pub async fn skill_has_permission(&self, skill_id: &str, permission: &str) -> bool {
        let row = sqlx::query(
            "SELECT permissions_granted, enabled FROM skills_installed WHERE skill_id = ?",
        )
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let Some(row) = row else {
            return false;
        };
        if row.get::<i64, _>("enabled") == 0 {
            return false;
        }
        let granted_json: String = row.get("permissions_granted");
        let granted: Vec<String> = serde_json::from_str(&granted_json).unwrap_or_default();
        granted.iter().any(|p| p == permission)
    }

    pub async fn uninstall_skill(&self, skill_id: &str) -> Result<(), String> {
        let res = sqlx::query("DELETE FROM skills_installed WHERE skill_id = ?")
            .bind(skill_id)
            .execute(&self.pool)
            .await
            .map_err(|_| "No pudimos desinstalar la habilidad.".to_string())?;
        if res.rows_affected() == 0 {
            return Err("Esa habilidad no estaba instalada.".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn rejects_unknown_permission_outside_closed_allowlist() {
        let bad = r#"
id = "peligrosa"
name = "Peligrosa"
description = "Intenta pedir root"
version = "1.0.0"
permissions = ["sistema:root"]
[[tools]]
name = "todo"
description = "nada"
"#;
        assert!(SkillManifest::from_toml_str(bad).is_err());
    }

    #[tokio::test]
    async fn skill_lifecycle_install_enforce_permissions_and_uninstall() {
        let db = Database::connect_in_memory().await.unwrap();
        let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let market = MarketService::from_dir(&db, &skills_dir);

        let skills = market.list_skills().await.unwrap();
        assert!(skills.len() >= 2);

        // Sin aprobar permisos falla.
        assert!(
            market
                .install_skill("jellyfin-ayudante", &[])
                .await
                .is_err()
        );

        // Con el permiso aprobado instala.
        let installed = market
            .install_skill("jellyfin-ayudante", &["app:jellyfin:api".to_string()])
            .await
            .unwrap();
        assert!(installed.installed);

        // Puede usar su permiso declarado, pero NUNCA salir de sus permisos.
        assert!(
            market
                .skill_has_permission("jellyfin-ayudante", "app:jellyfin:api")
                .await
        );
        assert!(
            !market
                .skill_has_permission("jellyfin-ayudante", "apps:instalar")
                .await
        );
        assert!(
            !market
                .skill_has_permission("jellyfin-ayudante", "fotos:leer")
                .await
        );

        market.uninstall_skill("jellyfin-ayudante").await.unwrap();
        assert!(
            !market
                .skill_has_permission("jellyfin-ayudante", "app:jellyfin:api")
                .await
        );
    }
}
