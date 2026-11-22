use std::collections::HashSet;
use std::time::Duration;

use serde::Deserialize;

use super::manifest::{AppCategory, AppManifest};

/// API pública oficial de LinuxServer.io (200+ imágenes multi-arquitectura `amd64` + `arm64`).
pub const LINUXSERVER_API_URL: &str =
    "https://api.linuxserver.io/api/v1/images?include_config=false&include_deprecated=false";

/// Catálogo comunitario `Lissy93/portainer-templates` (licencia MIT, 450+ apps con logos y descripciones).
pub const PORTAINER_TEMPLATES_URL: &str =
    "https://raw.githubusercontent.com/Lissy93/portainer-templates/main/templates.json";

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

#[derive(Debug, Deserialize)]
struct PortainerCatalog {
    #[serde(default)]
    templates: Vec<PortainerTemplate>,
}

#[derive(Debug, Deserialize)]
struct PortainerTemplate {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    logo: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    ports: Vec<String>,
    #[serde(default)]
    webpage: String,
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
        || lower.contains("stream")
        || lower.contains("game")
    {
        AppCategory::Peliculas
    } else if lower.contains("cloud")
        || lower.contains("file")
        || lower.contains("sync")
        || lower.contains("document")
        || lower.contains("productivity")
        || lower.contains("note")
        || lower.contains("wiki")
    {
        AppCategory::Archivos
    } else if lower.contains("home") || lower.contains("automation") || lower.contains("iot") {
        AppCategory::Casa
    } else if lower.contains("security")
        || lower.contains("password")
        || lower.contains("auth")
        || lower.contains("vpn")
        || lower.contains("privacy")
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

fn parse_port_spec(ports: &[String]) -> Option<u16> {
    for p in ports {
        let cleaned = p.split('/').next().unwrap_or(p);
        let candidate = cleaned.split(':').next().unwrap_or(cleaned);
        if let Ok(num) = candidate.trim().parse::<u16>()
            && num > 0
        {
            return Some(num);
        }
    }
    None
}

fn clean_text(raw: &str, max_chars: usize) -> String {
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

fn slugify(raw: &str) -> String {
    let without_paren = raw.split('(').next().unwrap_or(raw).trim();
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in without_paren.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
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

/// Descarga y combina los dos catálogos públicos open-source más grandes:
/// 1. **LinuxServer.io** (~200 apps multi-arquitectura con versiones fijadas y logos).
/// 2. **Lissy93/portainer-templates** (licencia MIT, ~450 apps con imágenes, logos y descripciones).
pub async fn fetch_live_catalog() -> Result<Vec<(AppManifest, String)>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Roost/0.1 (+https://github.com/Franco-Senes/Roost)")
        .build()
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    let mut seen = HashSet::new();

    // 1. LinuxServer.io (prioridad alta: versiones fijadas y soporte arm64/amd64)
    if let Ok(resp) = client.get(LINUXSERVER_API_URL).send().await
        && let Ok(ok) = resp.error_for_status()
        && let Ok(parsed) = ok.json::<ApiResponse>().await
    {
        let mut items = parsed.data.repositories.linuxserver;
        items.sort_by_key(|a| std::cmp::Reverse(a.monthly_pulls));

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
                || !seen.insert(id.clone())
            {
                continue;
            }

            let desc = clean_text(&item.description, 200);
            let tagline = clean_text(&item.description, 72);
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
            result.push((
                AppManifest {
                    id: id.clone(),
                    name: title_case(&id),
                    tagline: if tagline.is_empty() {
                        "Aplicación verificada de LinuxServer.io".to_string()
                    } else {
                        tagline
                    },
                    description: if desc.is_empty() {
                        "Aplicación multi-arquitectura disponible en LinuxServer.io.".to_string()
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
                },
                image_ref,
            ));
        }
    }

    // 2. Lissy93/portainer-templates (licencia MIT, cientos de apps con logo y descripción)
    if let Ok(resp) = client.get(PORTAINER_TEMPLATES_URL).send().await
        && let Ok(ok) = resp.error_for_status()
        && let Ok(parsed) = ok.json::<PortainerCatalog>().await
    {
        for tpl in parsed.templates {
            if tpl.image.trim().is_empty()
                || tpl.description.trim().is_empty()
                || !tpl.logo.starts_with("https://")
            {
                continue;
            }
            let id = slugify(&tpl.title);
            if id.len() < 2 || !seen.insert(id.clone()) {
                continue;
            }

            let title = tpl
                .title
                .split('(')
                .next()
                .unwrap_or(&tpl.title)
                .trim()
                .to_string();
            let desc = clean_text(&tpl.description, 200);
            let tagline = clean_text(&tpl.description, 72);
            let cat_joined = tpl.categories.join(",");
            let web_port = parse_port_spec(&tpl.ports).unwrap_or_else(|| known_port(&id));
            let version = tpl
                .image
                .rsplit_once(':')
                .map(|(_, tag)| tag.to_string())
                .unwrap_or_else(|| "stable".to_string());
            let upstream = if tpl.webpage.starts_with("https://") {
                tpl.webpage
            } else {
                "https://github.com/Lissy93/portainer-templates".to_string()
            };

            result.push((
                AppManifest {
                    id,
                    name: if title.is_empty() {
                        "App".to_string()
                    } else {
                        title
                    },
                    tagline,
                    description: desc,
                    category: map_category(&cat_joined),
                    version,
                    web_port,
                    ram_recommended_mb: 512,
                    icon: "remote".to_string(),
                    icon_url: Some(tpl.logo),
                    license: "Open Source (MIT Catalog)".to_string(),
                    upstream,
                },
                tpl.image,
            ));
        }
    }

    if result.is_empty() {
        return Err("No pudimos descargar el catálogo online en este momento.".to_string());
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

    #[test]
    fn slugify_normalizes_template_titles() {
        assert_eq!(slugify("AdGuard Home (container)"), "adguard-home");
        assert_eq!(slugify("Paperless-ngx"), "paperless-ngx");
    }
}
