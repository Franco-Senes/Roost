use std::time::Duration;

use serde::Deserialize;

use super::manifest::{AppCategory, AppManifest};

/// API pública y gratuita de LinuxServer.io (imágenes multi-arquitectura `amd64` + `arm64`).
pub const LINUXSERVER_API_URL: &str =
    "https://api.linuxserver.io/api/v1/images?include_config=false&include_deprecated=false";

#[derive(Debug, Deserialize)]
struct ApiResponse {
    data: ApiData,
}

#[derive(Debug, Deserialize)]
struct ApiData {
    repositories: ApiRepos,
}

#[derive(Debug, Deserialize)]
struct ApiRepos {
    #[serde(default)]
    linuxserver: Vec<RemoteImage>,
}

#[derive(Debug, Deserialize)]
struct RemoteImage {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    project_url: String,
    #[serde(default)]
    project_logo: String,
    #[serde(default)]
    stable: bool,
    #[serde(default)]
    deprecated: bool,
    #[serde(default)]
    monthly_pulls: u64,
}

fn map_category(raw: &str) -> AppCategory {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("photo") || lower.contains("image") || lower.contains("gallery") {
        AppCategory::Fotos
    } else if lower.contains("media")
        || lower.contains("video")
        || lower.contains("music")
        || lower.contains("book")
        || lower.contains("tv")
    {
        AppCategory::Peliculas
    } else if lower.contains("cloud")
        || lower.contains("file")
        || lower.contains("sync")
        || lower.contains("document")
        || lower.contains("productivity")
    {
        AppCategory::Archivos
    } else if lower.contains("home") || lower.contains("automation") || lower.contains("iot") {
        AppCategory::Casa
    } else if lower.contains("security")
        || lower.contains("password")
        || lower.contains("auth")
        || lower.contains("vpn")
    {
        AppCategory::Seguridad
    } else {
        AppCategory::Red
    }
}

fn known_port(name: &str) -> u16 {
    match name {
        "plex" => 32400,
        "immich" => 2283,
        "audiobookshelf" => 13378,
        "navidrome" => 4533,
        "syncthing" => 8384,
        "paperless-ngx" => 8000,
        "calibre-web" => 8083,
        "freshrss" => 8084,
        "qbittorrent" => 8085,
        "sonarr" => 8989,
        "radarr" => 7878,
        "prowlarr" => 9696,
        "bazarr" => 6767,
        "overseerr" => 5055,
        "uptime-kuma" => 3001,
        "filebrowser" => 8086,
        "photoprism" => 2342,
        "kavita" => 5000,
        _ => 8080,
    }
}

fn clean_text(raw: &str, max_chars: usize) -> String {
    // Quita enlaces Markdown básicos `[Texto](url)` -> `Texto` y espacios extra.
    let mut out = String::with_capacity(raw.len().min(max_chars + 16));
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            continue;
        }
        if c == ']' && chars.peek() == Some(&'(') {
            chars.next();
            for inner in chars.by_ref() {
                if inner == ')' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
        if out.chars().count() >= max_chars {
            break;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn title_case(id: &str) -> String {
    id.split('-')
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Descarga el catálogo público en vivo de LinuxServer.io y devuelve `(AppManifest, image_ref)`.
///
/// Solo incluye apps estables, no obsoletas y con versión fijada (nunca `latest`).
pub async fn fetch_live_catalog() -> Result<Vec<(AppManifest, String)>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Roost/0.1 (+https://github.com/Franco-Senes/Roost)")
        .build()
        .map_err(|e| e.to_string())?;

    let parsed: ApiResponse = client
        .get(LINUXSERVER_API_URL)
        .send()
        .await
        .map_err(|e| format!("No pudimos conectar con el catálogo online: {e}"))?
        .error_for_status()
        .map_err(|e| format!("El catálogo online respondió con error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("No pudimos leer el catálogo online: {e}"))?;

    let mut items = parsed.data.repositories.linuxserver;
    // Ordenamos por popularidad real (descargas mensuales) y nos quedamos con las 60 principales.
    items.sort_by_key(|a| std::cmp::Reverse(a.monthly_pulls));

    let mut result = Vec::new();
    for item in items {
        if !item.stable || item.deprecated {
            continue;
        }
        let version = item.version.trim();
        if version.is_empty() || version.eq_ignore_ascii_case("latest") {
            continue;
        }
        let id = item.name.trim().to_ascii_lowercase();
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            continue;
        }

        let desc = clean_text(&item.description, 180);
        let tagline = clean_text(&item.description, 68);
        let upstream = if item.project_url.starts_with("https://") {
            item.project_url
        } else {
            format!("https://docs.linuxserver.io/images/docker-{id}")
        };
        let icon_url = if item.project_logo.starts_with("https://") {
            Some(item.project_logo)
        } else {
            None
        };

        let image_ref = format!("lscr.io/linuxserver/{id}:{version}");
        let manifest = AppManifest {
            id: id.clone(),
            name: title_case(&id),
            tagline: if tagline.is_empty() {
                "Aplicación comunitaria verificada".to_string()
            } else {
                tagline
            },
            description: if desc.is_empty() {
                "Aplicación disponible desde el catálogo comunitario LinuxServer.io.".to_string()
            } else {
                desc
            },
            category: map_category(&item.category),
            version: version.to_string(),
            web_port: known_port(&id),
            ram_recommended_mb: 512,
            icon: "remote".to_string(),
            icon_url,
            license: "Open Source (LinuxServer.io)".to_string(),
            upstream,
        };
        result.push((manifest, image_ref));
        if result.len() >= 60 {
            break;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_strips_markdown_links() {
        let out = clean_text("[Plex](https://plex.tv) streams movies cleanly.", 100);
        assert_eq!(out, "Plex streams movies cleanly.");
    }

    #[test]
    fn maps_categories_to_roost_spanish_categories() {
        assert_eq!(map_category("Media Servers,Music"), AppCategory::Peliculas);
        assert_eq!(map_category("Photo Management"), AppCategory::Fotos);
        assert_eq!(map_category("Home Automation"), AppCategory::Casa);
        assert_eq!(map_category("Password Managers"), AppCategory::Seguridad);
    }
}
