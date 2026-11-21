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

## Eventos en tiempo real (SSE)

### `GET /api/events`
Abre un flujo `text/event-stream` (requiere sesión). Roost solo envía mensajes cuando ocurre un cambio real (sin polling de fondo).
