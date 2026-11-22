# API HTTP de Roost

Todas las rutas de datos viven bajo `/api` y devuelven JSON (salvo `/api/events`, que usa Server-Sent Events).

## Autenticación, sesiones y CSRF (M4)

Todas las rutas exigen una cookie de sesión `roost_session` (`HttpOnly`, `SameSite=Strict`, `Secure` cuando `ROOST_HTTPS=1`, válida por 30 días), salvo el primer arranque (`/api/setup`) y el inicio de sesión (`/api/login`).

Toda operación que modifica datos (`POST`, `PATCH`, `DELETE` con sesión) exige el encabezado `X-CSRF-Token` con el valor devuelto por `/api/setup`, `/api/login` o `/api/me`.

### `GET /api/setup`
Indica si falta crear al primer administrador: `{ "needs_setup": true }`.

### `POST /api/setup`
Crea el primer usuario con rol `admin` (contraseña de al menos 10 caracteres, guardada con `argon2id`) e inicia su sesión.

### `POST /api/login`
Inicia sesión con `{ "name": "...", "password": "..." }`. Limita a 5 intentos fallidos cada 15 minutos por IP y por usuario (`429 Too Many Requests`).

### `POST /api/logout`
Cierra la sesión actual (requiere `X-CSRF-Token`) y borra la cookie.

### `GET /api/me`
Devuelve `{ "id", "name", "role", "csrf_token" }` de la persona conectada.

## Usuarios (`admin`) (M4)

- `GET /api/users`: lista todos los usuarios (sin exponer contraseñas).
- `POST /api/users`: crea un miembro o administrador (`X-CSRF-Token`, contraseña >= 10 caracteres).
- `PATCH /api/users/{id}`: cambia `role` o `suspended` (`X-CSRF-Token`). Suspender revoca las sesiones activas de inmediato.
- `DELETE /api/users/{id}`: elimina un usuario (`X-CSRF-Token` y `{ "confirm": true }`).

## Tienda y apps (M3)

- `GET /api/store`: devuelve el catálogo completo de apps disponibles (requiere sesión).
- `GET /api/apps`: devuelve las apps instaladas y su estado actual (requiere sesión).
- `(admin)` `POST /api/apps/{id}/install`: inicia la instalación en segundo plano (`202 Accepted`), devuelve `ram_warning` si falta memoria y emite progreso real por SSE (`X-CSRF-Token`).
- `(admin)` `POST /api/apps/{id}/start`, `/stop`, `/restart`: controlan una app instalada (`X-CSRF-Token`).
- `(admin)` `DELETE /api/apps/{id}`: desinstala una app instalada (`X-CSRF-Token` y `{ "confirm": true }`).

## Marketplace (M10)

- `GET /api/market/models?scope=local|cloud&free=true|false`: modelos reales de proveedores con indicador `fits_ram` para Ollama.
- `GET /api/market/skills`: habilidades declarativas (`skills/<id>/skill.toml`) con explicación de permisos en español.
- `(admin)` `POST /api/market/skills/{id}/install` (`{ "approved_permissions": [...] }`, `X-CSRF-Token`) y `DELETE /api/market/skills/{id}`.

## Agente de IA y proyectos locales 24/7 (M9)

- `GET/PUT /api/ai/providers`: guarda claves cifradas con `chacha20poly1305` (solo devuelve `key_last4`).
- `GET /api/ai/models`: lista modelos de Ollama (local por defecto), OpenRouter y Hack Club AI.
- `POST /api/ai/chat`: stream SSE con `cloud_notice`, `reasoning`, `content` y tarjetas `action`.
- `POST /api/ai/actions/{id}/approve` y `/reject`: aprueba o rechaza acciones propuestas (exige `admin` para acciones de administración).
- `(admin)` `GET/POST /api/ai/projects`, `POST /api/ai/projects/{id}/start`, `/stop`, `DELETE /api/ai/projects/{id}`, `GET /api/ai/projects/{id}/logs`: gestiona proyectos locales de Antigravity (`agy`), Claude (`claude`) o comandos propios 24/7.

## Fotos, videos y álbumes (M7)

Todas las rutas exigen sesión y respetan privacidad estricta: cada persona solo ve sus propias fotos y álbumes compartidos con ella (el administrador no puede ver fotos privadas ajenas).

- `POST /api/photos`: subida por partes retomable (`X-CSRF-Token`, `X-Upload-Id`, `X-Chunk-Offset`, `X-Total-Size`, `X-Filename`, `X-Taken-At`). Deduplica por `blake3` y encola miniaturas WebP de 256 px y 1024 px.
- `GET /api/photos?cursor=&from=&to=&album=&trash=`: paginado por cursor ordenado por `taken_at DESC`.
- `GET /api/photos/{id}/thumb/{size}` (`256` o `1024`), `GET /api/photos/{id}/original`, `GET /api/photos/{id}/stream` (rangos HTTP `206 Partial Content`).
- `DELETE /api/photos/{id}` (mueve a papelera 30 días), `POST /api/photos/{id}/restore`.
- Álbumes: `GET/POST /api/albums`, `PATCH/DELETE /api/albums/{id}`, `POST /api/albums/{id}/share`, `POST /api/albums/{id}/photos`.

## Ajustes (M6)

- `GET /api/settings`: devuelve las preferencias guardadas (`theme`, `mode`, `server_name`) (requiere sesión).
- `PATCH /api/settings`: actualiza `theme` (`green` | `blue` | `purple`), `mode` (`light` | `dark`) o `server_name` (`X-CSRF-Token`).

## Sistema y métricas (M5)

### `GET /api/system`
Devuelve una foto instantánea de CPU, memoria, almacenamiento y temperatura (requiere sesión):

```json
{
  "cpu_percent": 12.4,
  "ram_used_mb": 1024,
  "ram_total_mb": 4096,
  "ram_available_mb": 3072,
  "disk_used_gb": 48.2,
  "disk_total_gb": 256.0,
  "temperature_c": 46.5,
  "demo": false
}
```

## Eventos en tiempo real (SSE)

### `GET /api/events`
Abre un flujo `text/event-stream` (requiere sesión). Roost solo envía mensajes cuando ocurre un cambio real y solo muestrea métricas (`system_metrics`) cada 2 segundos mientras haya al menos un navegador conectado. Con el panel cerrado el consumo de CPU queda en ~0%.
