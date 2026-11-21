# Formato del catálogo de apps (`apps/<id>/`)

Cada app disponible en la Tienda de Roost vive en su propia carpeta dentro de `apps/<id>/` y contiene tres archivos:

- `app.toml`: metadatos de la app para la Tienda y las validaciones de Roost.
- `compose.yaml`: definición de servicios con versión fijada y volúmenes contenidos.
- `icon.svg`: ícono vectorial propio o con licencia compatible con MIT.

## Campos de `app.toml`

```toml
id = "jellyfin"
name = "Jellyfin"
tagline = "Tus películas en cualquier pantalla"
description = "Mirá tus películas y series en la tele, la compu o el celular."
category = "Películas"     # Fotos | Películas | Archivos | Red | Casa | Seguridad
version = "10.10.7"        # versión fijada; nunca "latest"
web_port = 8096            # puerto donde se abre la interfaz web
ram_recommended_mb = 1024  # RAM recomendada para avisar antes de instalar
icon = "icon.svg"          # archivo relativo dentro de apps/<id>/
license = "GPL-2.0-only"   # identificador SPDX de la app upstream
upstream = "https://jellyfin.org"
```

## Reglas obligatorias del catálogo

1. **Español simple y sin jerga**: `tagline` y `description` usan español con voseo neutro y nunca mencionan palabras técnicas como "contenedor", "Docker", "puerto", "volumen" o "imagen".
2. **Versiones fijadas**: ni `app.toml` ni `compose.yaml` pueden usar `latest`. Todas las imágenes deben fijar su etiqueta de versión exacta.
3. **Volúmenes aislados**: todos los datos persistentes deben montarse dentro de `${ROOST_DATA_DIR:-/var/lib/roost}/apps/<id>/`.
4. **Sin modo privilegiado**: no se permite `privileged: true` salvo que sea imprescindible y esté explícitamente justificado en un comentario y en el PR.
5. **Etiquetas de Roost**: cada servicio en `compose.yaml` incluye las etiquetas `roost.managed=true` y `roost.app=<id>`.
6. **Licencias revisadas**: Roost no incluye código fuente de las apps del catálogo; solo referencia sus imágenes oficiales y documenta su licencia upstream en `license`. Los íconos propios de `apps/<id>/icon.svg` se distribuyen bajo licencia MIT junto con Roost.

## Aviso de RAM antes de instalar

Antes de iniciar una instalación, Roost compara `ram_recommended_mb` con la memoria libre del equipo. Si no alcanza, muestra un aviso claro en español explicando cuánta memoria recomienda la app y cuánta hay disponible.

## Apps iniciales del catálogo

| ID | Nombre | Categoría | Versión fijada | Licencia upstream | RAM recomendada |
| --- | --- | --- | --- | --- | --- |
| `jellyfin` | Jellyfin | Películas | `10.10.7` | `GPL-2.0-only` | 1024 MB |
| `adguard-home` | AdGuard Home | Red | `v0.107.61` | `GPL-3.0-only` | 256 MB |
| `nextcloud` | Nextcloud | Archivos | `30.0.10-apache` | `AGPL-3.0-only` | 1024 MB |
| `home-assistant` | Home Assistant | Casa | `2025.5.3` | `Apache-2.0` | 1024 MB |
| `vaultwarden` | Vaultwarden | Seguridad | `1.33.2` | `GPL-3.0-only` | 256 MB |
