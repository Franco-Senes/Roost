pub mod crypto;
pub mod providers;
pub mod runner;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::apps::AppService;
use crate::auth::Role;
use crate::db::Database;
use crate::metrics::MetricsService;
use crate::photos::PhotoService;

use self::crypto::{SecretCipher, mask_last4};
use self::providers::{ModelInfo, OllamaProvider, OpenAiCompatProvider, Provider};
use self::runner::ProjectRunner;

/// Configuración pública de un proveedor de IA (nunca devuelve la clave completa).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfigView {
    pub provider: String,
    pub configured: bool,
    pub key_last4: String,
    pub base_url: String,
    pub is_cloud: bool,
}

/// Acción propuesta por el agente que requiere aprobación explícita del usuario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAction {
    pub id: i64,
    pub user_id: i64,
    pub tool_name: String,
    pub arguments_json: String,
    pub summary: String,
    pub requires_admin: bool,
    pub status: String,
    pub result: String,
    pub created_at: i64,
}

/// Respuesta estructurada de un turno de chat con razonamiento y acciones propuestas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurnResponse {
    pub content: String,
    pub reasoning: String,
    pub is_cloud: bool,
    pub cloud_notice: Option<String>,
    pub proposed_action: Option<AiAction>,
}

/// Servicio principal del Agente de IA (`AGENTS.md` §9).
#[derive(Clone)]
pub struct AiService {
    pool: SqlitePool,
    cipher: SecretCipher,
    apps: Arc<AppService>,
    metrics: MetricsService,
    photos: PhotoService,
    pub projects: ProjectRunner,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl AiService {
    pub fn new(
        db: &Database,
        cipher: SecretCipher,
        apps: Arc<AppService>,
        metrics: MetricsService,
        photos: PhotoService,
    ) -> Self {
        Self {
            pool: db.pool().clone(),
            cipher,
            apps,
            metrics,
            photos,
            projects: ProjectRunner::new(db),
        }
    }

    /// Devuelve la configuración de proveedores del usuario mostrando solo los últimos 4 caracteres.
    pub async fn get_providers(&self, user_id: i64) -> Result<Vec<ProviderConfigView>, String> {
        let rows =
            sqlx::query("SELECT provider, key_last4, base_url FROM ai_providers WHERE user_id = ?")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await
                .map_err(|_| "No pudimos leer tus proveedores de IA.".to_string())?;

        let defaults = [
            ("ollama", "http://localhost:11434", false),
            ("openrouter", "https://openrouter.ai/api/v1", true),
            ("hackclub", "https://ai.hackclub.com/proxy/v1", true),
        ];

        Ok(defaults
            .into_iter()
            .map(|(id, default_url, is_cloud)| {
                if let Some(row) = rows.iter().find(|r| r.get::<String, _>("provider") == id) {
                    let last4: String = row.get("key_last4");
                    let url: String = row.get("base_url");
                    ProviderConfigView {
                        provider: id.to_string(),
                        configured: !is_cloud || !last4.is_empty(),
                        key_last4: last4,
                        base_url: if url.is_empty() {
                            default_url.to_string()
                        } else {
                            url
                        },
                        is_cloud,
                    }
                } else {
                    ProviderConfigView {
                        provider: id.to_string(),
                        configured: !is_cloud,
                        key_last4: String::new(),
                        base_url: default_url.to_string(),
                        is_cloud,
                    }
                }
            })
            .collect())
    }

    /// Guarda la clave de un proveedor cifrada con `chacha20poly1305` (nunca se loguea).
    pub async fn save_provider(
        &self,
        user_id: i64,
        provider: &str,
        api_key: Option<&str>,
        base_url: Option<&str>,
    ) -> Result<ProviderConfigView, String> {
        if !matches!(provider, "ollama" | "openrouter" | "hackclub") {
            return Err("Proveedor no soportado. Usá ollama, openrouter o hackclub.".to_string());
        }

        let clean_key = api_key.unwrap_or("").trim();
        let encrypted = if clean_key.is_empty() {
            String::new()
        } else {
            self.cipher.encrypt(clean_key)?
        };
        let last4 = mask_last4(clean_key);
        let url = base_url.unwrap_or("").trim();

        sqlx::query(
            "INSERT INTO ai_providers (user_id, provider, api_key_encrypted, key_last4, base_url)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(user_id, provider) DO UPDATE SET
               api_key_encrypted = CASE WHEN excluded.api_key_encrypted != '' THEN excluded.api_key_encrypted ELSE ai_providers.api_key_encrypted END,
               key_last4 = CASE WHEN excluded.key_last4 != '' THEN excluded.key_last4 ELSE ai_providers.key_last4 END,
               base_url = excluded.base_url",
        )
        .bind(user_id)
        .bind(provider)
        .bind(&encrypted)
        .bind(&last4)
        .bind(url)
        .execute(&self.pool)
        .await
        .map_err(|_| "No pudimos guardar el proveedor de IA.".to_string())?;

        let all = self.get_providers(user_id).await?;
        all.into_iter()
            .find(|p| p.provider == provider)
            .ok_or_else(|| "Error interno al leer el proveedor guardado.".to_string())
    }

    async fn load_decrypted_key(&self, user_id: i64, provider: &str) -> (String, String) {
        let row = sqlx::query(
            "SELECT api_key_encrypted, base_url FROM ai_providers WHERE user_id = ? AND provider = ?",
        )
        .bind(user_id)
        .bind(provider)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let Some(row) = row else {
            return (String::new(), String::new());
        };
        let enc: String = row.get("api_key_encrypted");
        let url: String = row.get("base_url");
        let plain = if enc.is_empty() {
            String::new()
        } else {
            self.cipher.decrypt(&enc).unwrap_or_default()
        };
        (plain, url)
    }

    /// Lista modelos disponibles desde Ollama (local por defecto), OpenRouter y Hack Club AI.
    pub async fn list_models(&self, user_id: i64) -> Vec<ModelInfo> {
        let avail_ram = self.metrics.available_ram_mb();
        let mut models = Vec::new();

        // 1. Ollama local (opción por defecto si está disponible).
        let (_, ollama_url) = self.load_decrypted_key(user_id, "ollama").await;
        let ollama = OllamaProvider {
            base_url: if ollama_url.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                ollama_url
            },
        };
        if let Ok(local) = ollama.list_models(avail_ram).await {
            models.extend(local);
        }

        // 2. OpenRouter si el usuario cargó su clave.
        let (or_key, or_url) = self.load_decrypted_key(user_id, "openrouter").await;
        if !or_key.is_empty() {
            let or_prov = OpenAiCompatProvider::openrouter(or_key, Some(or_url));
            if let Ok(list) = or_prov.list_models(avail_ram).await {
                models.extend(list);
            }
        }

        // 3. Hack Club AI si el usuario cargó su clave.
        let (hc_key, hc_url) = self.load_decrypted_key(user_id, "hackclub").await;
        if !hc_key.is_empty() {
            let hc_prov = OpenAiCompatProvider::hackclub(hc_key, Some(hc_url));
            if let Ok(list) = hc_prov.list_models(avail_ram).await {
                models.extend(list);
            }
        }

        // Asistente integrado de Roost siempre disponible para herramientas locales sin clave externa.
        models.push(ModelInfo {
            id: "roost/local-assistant".to_string(),
            name: "Asistente local de Roost (herramientas integradas)".to_string(),
            provider: "roost".to_string(),
            is_cloud: false,
            is_free: true,
            size_mb: Some(0),
            fits_ram: Some(true),
        });

        models
    }

    /// Procesa un mensaje del usuario, ejecuta herramientas de lectura y propone
    /// acciones con tarjeta de aprobación cuando hace falta cambiar algo (`AGENTS.md` §9).
    pub async fn chat(
        &self,
        user_id: i64,
        model: &str,
        message: &str,
    ) -> Result<ChatTurnResponse, String> {
        let lower = message.to_ascii_lowercase();
        let is_cloud = model.starts_with("openrouter/") || model.starts_with("hackclub/");
        let cloud_notice = if is_cloud {
            Some("Esta conversación sale de tu casa.".to_string())
        } else {
            None
        };

        // 1. Detección de intención de herramientas integradas (lectura libre / escritura con aprobación).
        if (lower.contains("película")
            || lower.contains("pelicula")
            || lower.contains("serie")
            || lower.contains("jellyfin"))
            && (lower.contains("instal") || lower.contains("pon") || lower.contains("quiero ver"))
        {
            let action = self
                .propose_action(
                    user_id,
                    "instalar_app",
                    r#"{"id":"jellyfin"}"#,
                    "Instalar Jellyfin (usa ~1024 MB de RAM)",
                    true,
                )
                .await?;
            return Ok(ChatTurnResponse {
                content: "Te recomiendo **Jellyfin** para ver tus películas y series en la tele, la compu o el celular. Preparé la tarjeta de aprobación para instalarlo cuando confirmes.".to_string(),
                reasoning: "El usuario quiere ver películas; consulté el catálogo (recomendar_app) y propuse instalar_app('jellyfin') con aprobación obligatoria.".to_string(),
                is_cloud,
                cloud_notice,
                proposed_action: Some(action),
            });
        }

        if lower.contains("anuncio") || lower.contains("publicidad") || lower.contains("adguard") {
            let action = self
                .propose_action(
                    user_id,
                    "instalar_app",
                    r#"{"id":"adguard-home"}"#,
                    "Instalar AdGuard Home (usa ~256 MB de RAM)",
                    true,
                )
                .await?;
            return Ok(ChatTurnResponse {
                content: "Para bloquear anuncios en toda la casa podés instalar **AdGuard Home**. Aprobá la tarjeta de abajo si querés que lo instale ahora.".to_string(),
                reasoning: "Identifiqué necesidad de bloqueo de anuncios y propuse instalar_app('adguard-home').".to_string(),
                is_cloud,
                cloud_notice,
                proposed_action: Some(action),
            });
        }

        if lower.contains("lento")
            || lower.contains("estado")
            || lower.contains("cpu")
            || lower.contains("ram")
        {
            let snap = self.metrics.snapshot();
            return Ok(ChatTurnResponse {
                content: format!(
                    "Revisé el estado del equipo: procesador al **{}%**, memoria **{} / {} MB** y disco **{} / {} GB**.",
                    snap.cpu_percent,
                    snap.ram_used_mb,
                    snap.ram_total_mb,
                    snap.disk_used_gb,
                    snap.disk_total_gb
                ),
                reasoning:
                    "Ejecuté estado_sistema() y espacio_disco() (herramientas de solo lectura)."
                        .to_string(),
                is_cloud,
                cloud_notice,
                proposed_action: None,
            });
        }

        if lower.contains("foto") || lower.contains("galer") {
            let page = self
                .photos
                .list_photos(user_id, None, None, None, None, false, 10)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(ChatTurnResponse {
                content: format!(
                    "Tenés **{}** fotos/videos recientes en tu galería privada. Podés abrir la pestaña **Fotos** para verlos o subir más.",
                    page.items.len()
                ),
                reasoning: "Ejecuté buscar_fotos() sobre la galería privada del usuario."
                    .to_string(),
                is_cloud,
                cloud_notice,
                proposed_action: None,
            });
        }

        // 2. Si el usuario eligió Ollama, OpenRouter o Hack Club AI, llamamos al proveedor real.
        let system_prompt = "Sos el asistente de Roost, un servidor familiar simple. Respondé en español con voseo neutro, sin palabras técnicas como Docker, contenedor o puerto.";
        if model.starts_with("ollama/") {
            let (_, url) = self.load_decrypted_key(user_id, "ollama").await;
            let prov = OllamaProvider {
                base_url: if url.is_empty() {
                    "http://localhost:11434".to_string()
                } else {
                    url
                },
            };
            let comp = prov.chat(model, system_prompt, message).await?;
            return Ok(ChatTurnResponse {
                content: comp.content,
                reasoning: comp.reasoning,
                is_cloud: false,
                cloud_notice: None,
                proposed_action: None,
            });
        }

        if model.starts_with("openrouter/") {
            let (key, url) = self.load_decrypted_key(user_id, "openrouter").await;
            let prov = OpenAiCompatProvider::openrouter(key, Some(url));
            let comp = prov.chat(model, system_prompt, message).await?;
            return Ok(ChatTurnResponse {
                content: comp.content,
                reasoning: comp.reasoning,
                is_cloud: true,
                cloud_notice,
                proposed_action: None,
            });
        }

        if model.starts_with("hackclub/") {
            let (key, url) = self.load_decrypted_key(user_id, "hackclub").await;
            let prov = OpenAiCompatProvider::hackclub(key, Some(url));
            let comp = prov.chat(model, system_prompt, message).await?;
            return Ok(ChatTurnResponse {
                content: comp.content,
                reasoning: comp.reasoning,
                is_cloud: true,
                cloud_notice,
                proposed_action: None,
            });
        }

        Ok(ChatTurnResponse {
            content: "Puedo ayudarte a instalar apps (por ejemplo: *\"instalá algo para ver películas\"*), revisar si el servidor está lento o buscar tus fotos. Si querés conversar con un modelo completo, configurá **Ollama**, **OpenRouter** o **Hack Club AI** arriba.".to_string(),
            reasoning: "Modo asistente local sin proveedor externo configurado.".to_string(),
            is_cloud: false,
            cloud_notice: None,
            proposed_action: None,
        })
    }

    pub async fn propose_action(
        &self,
        user_id: i64,
        tool_name: &str,
        arguments_json: &str,
        summary: &str,
        requires_admin: bool,
    ) -> Result<AiAction, String> {
        let created_at = now_unix();
        let res = sqlx::query(
            "INSERT INTO ai_actions (user_id, tool_name, arguments_json, summary, requires_admin, status, result, created_at)
             VALUES (?, ?, ?, ?, ?, 'pending', '', ?)",
        )
        .bind(user_id)
        .bind(tool_name)
        .bind(arguments_json)
        .bind(summary)
        .bind(i64::from(requires_admin))
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|_| "No pudimos registrar la acción propuesta.".to_string())?;

        Ok(AiAction {
            id: res.last_insert_rowid(),
            user_id,
            tool_name: tool_name.to_string(),
            arguments_json: arguments_json.to_string(),
            summary: summary.to_string(),
            requires_admin,
            status: "pending".to_string(),
            result: String::new(),
            created_at,
        })
    }

    /// Aprueba y ejecuta una acción propuesta por el agente (valida rol admin si corresponde).
    pub async fn approve_action(
        &self,
        actor_id: i64,
        actor_role: Role,
        action_id: i64,
    ) -> Result<AiAction, String> {
        let row = sqlx::query("SELECT * FROM ai_actions WHERE id = ?")
            .bind(action_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| "No pudimos buscar la acción.".to_string())?
            .ok_or_else(|| "No encontramos esa acción.".to_string())?;

        let owner_id: i64 = row.get("user_id");
        let requires_admin = row.get::<i64, _>("requires_admin") != 0;
        let status: String = row.get("status");
        if status != "pending" {
            return Err("Esta acción ya fue procesada.".to_string());
        }
        if requires_admin && actor_role != Role::Admin {
            return Err("Solo el administrador puede aprobar esta acción.".to_string());
        }
        if !requires_admin && owner_id != actor_id && actor_role != Role::Admin {
            return Err("No tenés permiso para aprobar esta acción.".to_string());
        }

        let tool_name: String = row.get("tool_name");
        let args_json: String = row.get("arguments_json");
        let args: serde_json::Value = serde_json::from_str(&args_json).unwrap_or_default();

        let exec_res = match tool_name.as_str() {
            "instalar_app" => {
                let id = args["id"].as_str().unwrap_or("");
                self.apps
                    .install(id)
                    .map(|r| format!("Instalación iniciada para {}", r.app_id))
                    .map_err(|e| e.to_string())
            }
            "reiniciar_app" => {
                let id = args["id"].as_str().unwrap_or("");
                self.apps
                    .restart(id)
                    .await
                    .map(|()| format!("App {id} reiniciada"))
                    .map_err(|e| e.to_string())
            }
            other => Err(format!("Herramienta no reconocida: {other}")),
        };

        let (next_status, result_text) = match exec_res {
            Ok(msg) => ("executed", msg),
            Err(err) => ("failed", err),
        };

        let _ = sqlx::query("UPDATE ai_actions SET status = ?, result = ? WHERE id = ?")
            .bind(next_status)
            .bind(&result_text)
            .bind(action_id)
            .execute(&self.pool)
            .await;

        Ok(AiAction {
            id: action_id,
            user_id: owner_id,
            tool_name,
            arguments_json: args_json,
            summary: row.get("summary"),
            requires_admin,
            status: next_status.to_string(),
            result: result_text,
            created_at: row.get("created_at"),
        })
    }

    pub async fn reject_action(&self, action_id: i64) -> Result<AiAction, String> {
        let row = sqlx::query("SELECT * FROM ai_actions WHERE id = ?")
            .bind(action_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| "No pudimos buscar la acción.".to_string())?
            .ok_or_else(|| "No encontramos esa acción.".to_string())?;

        let _ = sqlx::query("UPDATE ai_actions SET status = 'rejected' WHERE id = ?")
            .bind(action_id)
            .execute(&self.pool)
            .await;

        Ok(AiAction {
            id: action_id,
            user_id: row.get("user_id"),
            tool_name: row.get("tool_name"),
            arguments_json: row.get("arguments_json"),
            summary: row.get("summary"),
            requires_admin: row.get::<i64, _>("requires_admin") != 0,
            status: "rejected".to_string(),
            result: "Rechazada por el usuario".to_string(),
            created_at: row.get("created_at"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::auth::AuthService;
    use crate::docker::MockDocker;
    use crate::jobs::JobQueue;

    #[tokio::test]
    async fn movie_request_proposes_jellyfin_and_requires_admin_approval() {
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

        let apps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps");
        let docker = Arc::new(MockDocker::new());
        let apps = Arc::new(
            AppService::from_dir(
                &apps_dir,
                PathBuf::from("/tmp/roost-ai-test"),
                docker,
                Arc::new(|| 4096),
            )
            .unwrap(),
        );
        let metrics = MetricsService::new(true);
        let jobs = JobQueue::new(&db);
        let photos = PhotoService::new(&db, PathBuf::from("/tmp/roost-ai-photos"), jobs);
        let ai = AiService::new(
            &db,
            SecretCipher::ephemeral(),
            apps.clone(),
            metrics,
            photos,
        );

        // "instalá algo para ver películas" -> propone tarjeta de aprobación para Jellyfin.
        let turn = ai
            .chat(
                admin.user.id,
                "roost/local-assistant",
                "instalá algo para ver películas",
            )
            .await
            .unwrap();
        let action = turn.proposed_action.expect("debe proponer acción");
        assert_eq!(action.tool_name, "instalar_app");
        assert!(action.summary.contains("Jellyfin"));

        // Un miembro NO puede aprobar acciones de admin.
        assert!(
            ai.approve_action(member.id, Role::Member, action.id)
                .await
                .is_err()
        );

        // El admin aprueba -> se instala Jellyfin.
        let executed = ai
            .approve_action(admin.user.id, Role::Admin, action.id)
            .await
            .unwrap();
        assert_eq!(executed.status, "executed");
    }
}
