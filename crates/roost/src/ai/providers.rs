use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Modelo disponible devuelto por un proveedor de IA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub is_cloud: bool,
    pub is_free: bool,
    pub size_mb: Option<u64>,
    pub fits_ram: Option<bool>,
}

/// Respuesta de chat del proveedor (texto + razonamiento opcional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletion {
    pub content: String,
    pub reasoning: String,
}

/// Trait común para proveedores de IA (`AGENTS.md` §9): Ollama, OpenRouter y Hack Club AI.
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn is_cloud(&self) -> bool;
    fn list_models(&self, available_ram_mb: u64) -> BoxFuture<'_, Result<Vec<ModelInfo>, String>>;
    fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_message: &str,
    ) -> BoxFuture<'_, Result<ChatCompletion, String>>;
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Roost/0.1")
        .build()
        .map_err(|e| e.to_string())
}

/// Proveedor local Ollama (`http://localhost:11434`).
pub struct OllamaProvider {
    pub base_url: String,
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct OllamaTagsResp {
    #[serde(default)]
    models: Vec<OllamaTag>,
}

#[derive(Deserialize)]
struct OllamaTag {
    name: String,
    #[serde(default)]
    size: u64,
}

impl Provider for OllamaProvider {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn is_cloud(&self) -> bool {
        false
    }

    fn list_models(&self, available_ram_mb: u64) -> BoxFuture<'_, Result<Vec<ModelInfo>, String>> {
        let base = self.base_url.trim_end_matches('/').to_string();
        Box::pin(async move {
            let client = http_client()?;
            let resp: OllamaTagsResp = client
                .get(format!("{base}/api/tags"))
                .timeout(Duration::from_secs(3))
                .send()
                .await
                .map_err(|e| format!("Ollama no responde en {base}: {e}"))?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            Ok(resp
                .models
                .into_iter()
                .map(|m| {
                    let size_mb = m.size / (1024 * 1024);
                    // Estimación conservadora: un modelo necesita ~1.25x su tamaño en RAM.
                    let needed_mb = size_mb.saturating_mul(5) / 4;
                    ModelInfo {
                        id: format!("ollama/{}", m.name),
                        name: m.name,
                        provider: "ollama".to_string(),
                        is_cloud: false,
                        is_free: true,
                        size_mb: Some(size_mb),
                        fits_ram: Some(needed_mb <= available_ram_mb),
                    }
                })
                .collect())
        })
    }

    fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_message: &str,
    ) -> BoxFuture<'_, Result<ChatCompletion, String>> {
        let base = self.base_url.trim_end_matches('/').to_string();
        let model = model.strip_prefix("ollama/").unwrap_or(model).to_string();
        let system_prompt = system_prompt.to_string();
        let user_message = user_message.to_string();
        Box::pin(async move {
            let client = http_client()?;
            let body = serde_json::json!({
                "model": model,
                "stream": false,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": user_message }
                ]
            });
            let json: serde_json::Value = client
                .post(format!("{base}/api/chat"))
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("No pudimos hablar con Ollama: {e}"))?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let content = json["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let reasoning = json["message"]["thinking"]
                .as_str()
                .unwrap_or("")
                .to_string();
            Ok(ChatCompletion { content, reasoning })
        })
    }
}

/// Proveedor compatible con OpenAI (usado por OpenRouter y Hack Club AI).
pub struct OpenAiCompatProvider {
    pub provider_id: &'static str,
    pub base_url: String,
    pub api_key: String,
}

impl OpenAiCompatProvider {
    pub fn openrouter(api_key: String, base_url: Option<String>) -> Self {
        Self {
            provider_id: "openrouter",
            base_url: base_url
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
            api_key,
        }
    }

    pub fn hackclub(api_key: String, base_url: Option<String>) -> Self {
        Self {
            provider_id: "hackclub",
            base_url: base_url
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "https://ai.hackclub.com/proxy/v1".to_string()),
            api_key,
        }
    }
}

impl Provider for OpenAiCompatProvider {
    fn id(&self) -> &'static str {
        self.provider_id
    }

    fn is_cloud(&self) -> bool {
        true
    }

    fn list_models(&self, _available_ram_mb: u64) -> BoxFuture<'_, Result<Vec<ModelInfo>, String>> {
        let base = self.base_url.trim_end_matches('/').to_string();
        let key = self.api_key.clone();
        let provider_id = self.provider_id;
        Box::pin(async move {
            let client = http_client()?;
            let mut req = client
                .get(format!("{base}/models"))
                .timeout(Duration::from_secs(6));
            if !key.is_empty() {
                req = req.bearer_auth(&key);
            }
            let json: serde_json::Value = req
                .send()
                .await
                .map_err(|e| format!("No pudimos consultar los modelos de {provider_id}: {e}"))?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let mut out = Vec::new();
            if let Some(arr) = json["data"].as_array() {
                for item in arr.iter().take(40) {
                    let Some(id) = item["id"].as_str() else {
                        continue;
                    };
                    let name = item["name"].as_str().unwrap_or(id).to_string();
                    let is_free = id.contains(":free")
                        || provider_id == "hackclub"
                        || item["pricing"]["prompt"].as_str() == Some("0");
                    out.push(ModelInfo {
                        id: format!("{provider_id}/{id}"),
                        name,
                        provider: provider_id.to_string(),
                        is_cloud: true,
                        is_free,
                        size_mb: None,
                        fits_ram: None,
                    });
                }
            }
            Ok(out)
        })
    }

    fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_message: &str,
    ) -> BoxFuture<'_, Result<ChatCompletion, String>> {
        let base = self.base_url.trim_end_matches('/').to_string();
        let key = self.api_key.clone();
        let provider_id = self.provider_id;
        let clean_model = model
            .strip_prefix(&format!("{provider_id}/"))
            .unwrap_or(model)
            .to_string();
        let system_prompt = system_prompt.to_string();
        let user_message = user_message.to_string();
        Box::pin(async move {
            if key.is_empty() {
                return Err(format!(
                    "Cargá primero tu clave de {provider_id} en los ajustes del Agente."
                ));
            }
            let client = http_client()?;
            let body = serde_json::json!({
                "model": clean_model,
                "messages": [
                    { "role": "system", "content": system_prompt },
                    { "role": "user", "content": user_message }
                ]
            });
            let json: serde_json::Value = client
                .post(format!("{base}/chat/completions"))
                .bearer_auth(&key)
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("No pudimos contactar a {provider_id}: {e}"))?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;

            let msg = &json["choices"][0]["message"];
            let content = msg["content"].as_str().unwrap_or("").to_string();
            let reasoning = msg["reasoning"]
                .as_str()
                .or_else(|| msg["reasoning_content"].as_str())
                .unwrap_or("")
                .to_string();
            Ok(ChatCompletion { content, reasoning })
        })
    }
}
