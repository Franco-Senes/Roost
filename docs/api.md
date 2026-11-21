# API HTTP de Roost

Todas las rutas de datos viven bajo `/api` y devuelven JSON (salvo `/api/events`, que usa Server-Sent Events).

> Nota: a partir de M4, todas las rutas exigirán sesión activa (y las de administración exigirán rol `admin`).

## Tienda y apps (M3)

### `GET /api/store`

Devuelve el catálogo completo de apps disponibles para instalar.

```json
[
  {
    "id": "jellyfin",
    "name": "Jellyfin",
    "tagline": "Tus películas en cualquier pantalla",
    "description": "Mirá tus películas, series y música desde la tele, la compu o el celular, todo guardado en tu casa.",
    "category": "Películas",
    "version": "10.10.7",
    "web_port": 8096,
    "ram_recommended_mb": 1024,
    "icon": "icon.svg",
    "license": "GPL-2.0-only",
    "upstream": "https://jellyfin.org"
  }
]
```

### `GET /api/apps`

Devuelve las apps instaladas junto con su estado actual (`running`, `stopped`, etc.).

### `POST /api/apps/{id}/install`

Inicia la instalación de una app en segundo plano y emite el avance real en MB por `GET /api/events`.

- Respuesta `202 Accepted`:

```json
{
  "app_id": "jellyfin",
  "status": "installing",
  "ram_warning": null
}
```

Si la memoria libre del equipo es menor a `ram_recommended_mb`, `ram_warning` incluye un aviso claro en español.

### `POST /api/apps/{id}/start`
### `POST /api/apps/{id}/stop`
### `POST /api/apps/{id}/restart`

Inician, detienen o reinician una app instalada y emiten un evento `app_state_changed` por SSE.

### `DELETE /api/apps/{id}`

Desinstala una app instalada. Por seguridad exige confirmación explícita en el cuerpo JSON:

```json
{ "confirm": true }
```

Si falta `confirm: true`, responde `400 Bad Request` con un mensaje claro en español.

## Eventos en tiempo real (SSE)

### `GET /api/events`

Abre un flujo `text/event-stream`. Roost solo envía mensajes cuando ocurre un cambio real (sin polling de fondo).

Ejemplos de eventos `data`:

```json
{"type":"app_install_progress","app_id":"jellyfin","phase":"downloading","downloaded_mb":50.0,"total_mb":100.0,"percent":50}
{"type":"app_install_finished","app_id":"jellyfin","status":"running"}
{"type":"app_state_changed","app_id":"jellyfin","status":"stopped"}
```
