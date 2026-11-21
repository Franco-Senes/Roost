# Roost: guía completa para el agente de programación

Sos el agente de programación de Roost. Repo: https://github.com/Franco-Senes/Roost

Este documento es la fuente de verdad del proyecto: qué es, cómo se construye, qué reglas no se rompen y en qué orden se hace. Si algo de lo que te piden contradice este archivo, preguntá a Franco antes de hacerlo.

En cada sesión:

1. Leé este archivo entero.
2. Leé `PROGRESS.md` para saber qué hitos están terminados.
3. Hacé un solo hito: el siguiente que falta.
4. Abrí el Pull Request y frená.

## 1. Qué es Roost

Roost convierte una computadora vieja, una mini PC o una Raspberry Pi en el servidor de una familia, sin que nadie tenga que saber Linux.

Las cuatro partes del producto:

- **Tienda de apps con un clic**: películas (Jellyfin), archivos (Nextcloud), bloqueo de anuncios (AdGuard Home), casa inteligente (Home Assistant), contraseñas (Vaultwarden) y más.
- **Fotos y videos integrados**: una galería propia dentro de Roost. Las fotos de la familia se guardan en casa, sin pagar por la nube y sin instalar ninguna app aparte.
- **Agente de IA propio con marketplace**: un asistente que ayuda a usar el servidor ("quiero ver películas en la tele", "¿por qué está lento?"). Funciona con Ollama (local), OpenRouter y Hack Club AI, y tiene un marketplace de modelos y de habilidades.
- **Emulador**: permite probar Roost entero sin instalarlo en la máquina.

Para quién es:

- **El admin**: la persona de la familia que instala Roost. Sabe usar una computadora, pero no sabe Linux.
- **Los usuarios**: el resto de la familia. Solo usan las fotos, las apps y el agente desde el teléfono o la tele.

Cómo tiene que sentirse: como un sistema operativo moderno y lindo, no como un panel técnico. Nunca le mostramos al usuario palabras como "contenedor", "puerto", "Docker", "volumen" o "imagen". Si algo falla, el mensaje explica qué pasó y qué hacer, en español simple.

Las cuatro promesas, en orden de prioridad:

1. **Liviano**: gasta poca RAM y poca CPU.
2. **Rápido**: todo responde al instante.
3. **Hermoso**: Material Design 3.
4. **Open source de verdad**: licencia MIT.

Inspiración: Franco ya hizo Samaipata (https://github.com/Franco-Senes/Samaipata), un cliente de IA en Node.js con:

- Hack Club AI y Ollama
- streaming por SSE que muestra el razonamiento del modelo
- marketplace de modelos con ranking
- panel de admin: el primer usuario es admin, con permisos y límites por modelo, estadísticas y suspensión de usuarios

Tomá esas funciones como referencia, pero Roost se escribe desde cero en Rust, con interfaz nueva. No copies código de Samaipata sin que Franco le agregue antes una licencia.

## 2. Rendimiento: la regla más importante

La interfaz Material corre en el navegador del dispositivo que abre Roost, no en el servidor. El servidor solo sirve la página y datos chicos. Por eso el backend puede y debe ser liviano.

| Qué | Límite / objetivo |
| --- | --- |
| RAM de Roost (sin contar las apps instaladas ni Ollama) | nunca más de 1 GB, ni siquiera procesando fotos y videos |
| RAM real | medila siempre y anotala en `BENCHMARKS.md`; el límite es un techo, no una meta a llenar |
| CPU sin hacer nada | cerca de 0%: nada de bucles ni polling de fondo |
| Binario | lo más chico posible; anotá el tamaño en cada hito |
| Interfaz, primera carga | menos de 250 KB comprimida |
| Panel visible en una Raspberry Pi 4, red local | menos de 1 segundo |
| Máquina recomendada | 4 GB de RAM o más; mínima, 2 GB |

Técnicas obligatorias:

- Perfil release: `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "abort"`, `opt-level = 3`.
- Nada de polling: el servidor empuja los cambios por Server-Sent Events (SSE) solo cuando pasa algo, y solo mientras alguien tiene la pantalla abierta.
- Una sola base de datos, SQLite, en modo WAL. Nada de Postgres, Redis ni otros servicios aparte.
- Cola de trabajos pesados (miniaturas, videos, indexado):
  - 1 o 2 trabajos a la vez, con baja prioridad (`nice`)
  - se pausa si la CPU pasa del 80% o la RAM de Roost se acerca al límite
  - procesa por lotes, nunca toda la galería de golpe
  - si se corta la luz, retoma donde quedó
- Carga perezosa:
  - El agente de IA está apagado por defecto y no reserva memoria hasta que se activa.
  - `ffmpeg` se ejecuta como proceso aparte solo cuando hace falta.
- Frontend:
  - cada pantalla carga solo su código (code splitting)
  - imágenes con `loading="lazy"`
  - miniaturas WebP de 256 px y 1024 px
  - listas largas virtualizadas: la galería no dibuja miles de fotos a la vez
- Streaming: archivos grandes (fotos, videos, subidas) se leen y escriben por partes, nunca enteros en memoria.
- Dependencias: antes de agregar una, pensá cuánto pesa y justificalo en el PR. Elegí la opción más liviana que funcione bien.

## 3. Arquitectura

```
Navegador (PC / teléfono / tele)
   │  HTTPS en la red local: páginas, API JSON y eventos SSE
   ▼
roost (un binario en Rust)
   ├── web        → sirve el frontend embebido y la API (axum)
   ├── auth       → usuarios, sesiones, roles, límite de intentos
   ├── apps       → catálogo, instalar, iniciar, detener, desinstalar
   ├── docker     → habla con Docker por su socket (bollard)
   ├── metrics    → CPU, RAM, disco, temperatura (sysinfo)
   ├── photos     → subida, galería, álbumes, papelera, streaming de video
   ├── jobs       → cola de trabajos pesados con prioridad baja
   ├── ai         → proveedores de IA, herramientas, conversaciones
   ├── market     → marketplace de modelos y de habilidades
   └── db         → SQLite (sqlx) y migraciones
```

Carpetas de datos. Por defecto en `/var/lib/roost`; se pueden configurar.

- `roost.db`: la base de datos
- `secret.key`: clave para cifrar los secretos, con permisos `0600`, creada en el primer arranque
- `photos/originals/<usuario>/<año>/<mes>/`: los originales, nunca se modifican
- `photos/thumbs/`: las miniaturas, que se pueden regenerar
- `apps/<id>/`: los datos de cada app instalada

Estructura del repo:

```
crates/roost/        # el binario
  src/{web,auth,apps,docker,metrics,photos,jobs,ai,market,db}/
  migrations/
apps/<id>/app.toml   # catálogo de apps
apps/<id>/compose.yaml
skills/<id>/skill.toml   # habilidades oficiales del agente
web/                 # frontend Svelte
docker/              # imagen del emulador
docs/                # solo lo necesario: formato de apps, de habilidades, API
AGENTS.md  PROGRESS.md  BENCHMARKS.md  README.md  LICENSE
```

Stack:

- Backend: Rust estable.
  - `tokio`, `axum`, `tower-http` (compresión y archivos)
  - `serde`, `toml`, `sqlx` (SQLite)
  - `bollard`, `sysinfo`, `argon2`, `chacha20poly1305` (cifrado de secretos), `blake3` (hash de fotos)
  - `image` (miniaturas WebP), `kamadak-exif` (fechas de las fotos)
  - `reqwest` con `rustls` (proveedores de IA), `tracing`
- Frontend: Svelte con Material Web (`@material/web`).
  - Importá solo los componentes que se usan.
  - El build se embebe en el binario.
  - PWA instalable.
- Videos: `ffmpeg` incluido en la imagen de Roost, para que el usuario no descargue nada extra. Revisá que su licencia permita distribuirlo así y documentalo.

## 4. Modelo de datos (SQLite)

Tablas mínimas. Podés agregar columnas si hace falta, explicándolo en el PR.

- `users`: `id`, `name`, `password_hash`, `role` (`admin`/`member`), `suspended`, `created_at`
- `sessions`: `id` (aleatorio, 256 bits), `user_id`, `expires_at`, `user_agent`
- `login_attempts`: para limitar los intentos por IP y por usuario
- `apps_installed`: `app_id`, `version`, `status`, `installed_at`, `installed_by`
- `photos`: `id`, `owner_id`, `hash` (blake3), `path`, `kind` (`photo`/`video`), `taken_at`, `width`, `height`, `duration`, `size`, `deleted_at` (papelera)
- `albums`, `album_photos`, `album_shares` (`album_id`, `user_id`, `can_edit`)
- `jobs`: `id`, `kind`, `payload`, `status`, `attempts`, `created_at`
- `ai_providers`: `user_id`, `provider`, `api_key_encrypted`, `base_url`
- `ai_model_rules`: `model`, `allowed_role` o `user_id`, `daily_limit`
- `ai_conversations`, `ai_messages` (incluye `reasoning` y `tool_calls`)
- `ai_actions`: cada acción que propuso el agente, si el usuario la aprobó o rechazó, y el resultado
- `skills_installed`: `skill_id`, `version`, `permissions_granted`, `enabled`
- `settings`: clave/valor (tema, idioma, nombre del servidor)

## 5. API

JSON bajo `/api`. Todas las rutas exigen sesión, salvo el primer arranque y el inicio de sesión. Las rutas marcadas `(admin)` exigen rol admin.

- Primer arranque y sesión:
  - `GET /api/setup`: dice si falta crear el admin
  - `POST /api/setup`: crea el admin
  - `POST /api/login`, `POST /api/logout`, `GET /api/me`
- Usuarios `(admin)`: `GET/POST /api/users`, `PATCH /api/users/:id`, `DELETE /api/users/:id`
- Sistema: `GET /api/system` (resumen) y `GET /api/events` (SSE con métricas, progreso de instalaciones y trabajos)
- Apps:
  - `GET /api/store`: el catálogo
  - `GET /api/apps`: las instaladas
  - `(admin)` `POST /api/apps/:id/install`, `/start`, `/stop`, `/restart`, `DELETE /api/apps/:id` (pide `confirm: true`)
- Fotos:
  - `POST /api/photos`: subida por partes, que se puede retomar
  - `GET /api/photos?cursor=&from=&to=&album=`: paginado por cursor
  - `GET /api/photos/:id/thumb/:size`, `GET /api/photos/:id/original`, `GET /api/photos/:id/stream` (rangos HTTP)
  - `DELETE /api/photos/:id`: va a la papelera; `POST /api/photos/:id/restore`
  - álbumes: `GET/POST /api/albums`, `PATCH/DELETE /api/albums/:id`, `POST /api/albums/:id/share`
- IA:
  - `GET/PUT /api/ai/providers`: claves propias, nunca devueltas completas
  - `GET /api/ai/models`
  - `POST /api/ai/chat`: respuesta por SSE, con texto, razonamiento y acciones propuestas
  - `POST /api/ai/actions/:id/approve` y `/reject`
- Marketplace:
  - `GET /api/market/models`, `GET /api/market/skills`
  - `(admin)` `POST /api/market/skills/:id/install` (con los permisos aprobados) y `DELETE /api/market/skills/:id`
- Ajustes: `GET/PATCH /api/settings`

Documentá la API en `docs/api.md` a medida que la construyas.

## 6. Formato de apps (`apps/<id>/app.toml`)

```toml
id = "jellyfin"
name = "Jellyfin"
tagline = "Tus películas en cualquier pantalla"   # español simple, sin palabras técnicas
description = "Mirá tus películas y series en la tele, la compu o el celular."
category = "Películas"     # Fotos | Películas | Archivos | Red | Casa | Seguridad
version = "10.x"           # versión fijada; nunca "latest"
web_port = 8096            # dónde se abre la app
ram_recommended_mb = 1024
icon = "icon.svg"          # ícono propio o con licencia que permita usarlo
license = "GPL-2.0"        # licencia de la app
upstream = "https://jellyfin.org"
```

Reglas del catálogo:

- versiones fijadas
- volúmenes dentro de `apps/<id>/`
- nada de modo privilegiado salvo que sea imprescindible y esté justificado

Antes de instalar, Roost compara `ram_recommended_mb` con la RAM libre y avisa en español si no alcanza.

## 7. Interfaz (Material Design 3)

Referencia visual: el lienzo de diseño de Franco (Inicio, Tienda y Teléfono en estilo Material).

Navegación:

- Computadora: menú lateral (navigation rail) con un botón grande "+" (Instalar) arriba y los destinos Inicio, Tienda, Fotos, Agente y Ajustes; la sección activa se marca con una pastilla de color.
- Teléfono: barra de navegación abajo con 5 destinos y un botón flotante para la acción principal de cada pantalla.

Pantallas:

- **Inicio**:
  - saludo y estado ("Todo funciona · 6 apps activas")
  - tarjetas de Sistema (CPU, memoria, temperatura) y Almacenamiento
  - instalación en curso, con MB reales y tiempo restante
  - "Tus apps"
- **Tienda**:
  - búsqueda "¿Qué querés hacer?"
  - filtros de categoría con ✓
  - tarjeta recomendada
  - tarjetas de app con Instalar, Abrir o el progreso
- **Fotos**:
  - línea de tiempo agrupada por mes, con desplazamiento rápido
  - álbumes y compartidos
  - visor a pantalla completa con deslizar
  - selección múltiple, papelera y botón de subir
- **Agente**:
  - chat con streaming y razonamiento plegable
  - selector de modelo con aviso "nube" o "local"
  - tarjetas de acción con Aprobar y Rechazar
  - acceso al marketplace
- **Ajustes**:
  - usuarios (admin), tema, claves de IA, almacenamiento, actualizaciones
  - "Acerca de", con la licencia y el aviso de que el proyecto se hace con ayuda de IA

Temas. Tokens de Material 3; cada tema trae modo claro y modo oscuro.

- **Verde (por defecto)**: `primary #3B6939`, `primaryContainer #BCF0B4`, `onPrimaryContainer #002204`, `secondaryContainer #D7E8CD`, `surface #F7FBF1`, `surfaceContainerHigh #E6E9E0`, `surfaceContainerHighest #E0E4DB`, `onSurface #181D17`, `onSurfaceVariant #424940`, `outlineVariant #C2C8BD`.
- **Azul**: `primary #415F91`, `primaryContainer #D6E3FF`, `onPrimaryContainer #001B3E`, `secondaryContainer #DAE2F9`, `surface #F9F9FF`.
- **Violeta**: `primary #6750A4`, `primaryContainer #EADDFF`, `onPrimaryContainer #21005D`, `secondaryContainer #E8DEF8`, `surface #FEF7FF`.
- Los colores del modo oscuro se generan con la misma lógica de Material (tonos altos sobre superficies oscuras), y el contraste se verifica.

Reglas de la interfaz:

- Tipografía Roboto, con la escala de Material: display, headline, title, body y label.
- Animaciones cortas con CSS (150–300 ms) que respetan `prefers-reduced-motion`.
- Accesible:
  - todo se usa con teclado
  - contraste de al menos 4,5:1
  - `aria-label` en los botones que solo tienen ícono
  - zonas táctiles de 44 px como mínimo
- Responsive: teléfono (menos de 600 px), tablet (600–1200 px) y computadora (más de 1200 px).
- Textos:
  - español con voseo neutro ("instalá", "probá")
  - sin palabras técnicas
  - los errores dicen qué pasó y qué hacer: no "Error 500", sino "No pudimos instalar Jellyfin porque falta espacio. Liberá 2 GB y probá de nuevo."
- Íconos: Material Symbols (licencia Apache 2.0), incluidos en el build, sin cargarlos de internet.

## 8. Fotos y videos (detalle)

- Subida:
  - por partes y retomable, desde la PWA y la computadora
  - detecta repetidas por hash blake3: si ya existe, no la guarda dos veces
- Originales intocables: nunca se recomprimen ni se modifican.
- Miniaturas: WebP de 256 px y 1024 px, generadas en la cola de baja prioridad. Mientras no existen, se muestra un color promedio.
- Fecha: se toma del EXIF; si no hay, de la fecha del archivo. La línea de tiempo se ordena por `taken_at`.
- Videos:
  - se reproducen con streaming por rangos HTTP
  - la miniatura y la conversión se hacen con `ffmpeg` en la cola, y solo se convierte si el navegador no puede reproducir el formato
- Privacidad: cada persona ve solo sus fotos y los álbumes que le compartieron. El admin no puede ver las fotos privadas de otros.
- Papelera: 30 días y después se borra sola. Borrar para siempre pide confirmación.
- Limitación honesta: la copia automática en segundo plano desde una PWA es limitada, sobre todo en iPhone. Documentalo en la interfaz y en `docs/`. No lo prometas.

## 9. Agente de IA (detalle)

Proveedores. Un trait `Provider` con `list_models()` y `chat_stream()`, y tres implementaciones:

- **Ollama**: local, `http://localhost:11434`; puede correr como app de la Tienda.
- **OpenRouter**: API compatible con OpenAI.
- **Hack Club AI**: compatible con OpenAI y con OpenRouter; base `https://ai.hackclub.com/proxy/v1`.

Claves:

- Cada usuario carga su propia clave. Roost nunca trae claves incluidas.
- Se guardan cifradas con `chacha20poly1305` usando `secret.key`.
- En la interfaz solo se ven los últimos 4 caracteres, y nunca aparecen en los logs.

Aviso de nube: si el modelo elegido es de OpenRouter o Hack Club AI, la interfaz muestra "Esta conversación sale de tu casa" antes del primer mensaje. Si Ollama está disponible, es la opción por defecto.

Herramientas. El modelo solo puede usar las herramientas que define el código, cada una con su esquema JSON:

- `recomendar_app(necesidad)`
- `instalar_app(id)`
- `reiniciar_app(id)`
- `estado_sistema()`
- `espacio_disco()`
- `buscar_fotos(desde, hasta, album)`

Flujo de una acción:

1. El modelo propone la acción.
2. Roost la guarda en `ai_actions` y la muestra como tarjeta: "Instalar Jellyfin (usa ~1 GB de RAM)", con Aprobar y Rechazar.
3. Solo si el usuario aprueba, se ejecuta.

Leer es libre; cambiar algo siempre pide aprobación. Un usuario que no es admin no puede aprobar acciones de admin.

Admin: decide qué modelos puede usar cada rol o usuario, define límites diarios y ve estadísticas de uso.

Rendimiento: el módulo de IA no reserva memoria hasta que se activa, y Roost funciona perfecto con el agente apagado.

## 10. Marketplace (detalle)

- Modelos:
  - la lista sale de la API de cada proveedor
  - filtros: local o nube, gratis o pago, tamaño
  - para Ollama, marca qué modelos entran en la RAM libre de esa máquina
  - los rankings solo aparecen si vienen de una fuente real nombrada, con link
- Habilidades: paquetes que suman herramientas al agente. En la primera versión son declarativas, sin código propio: cada herramienta de una habilidad combina acciones que Roost ya tiene, o llama a la API de una app instalada.

```toml
id = "jellyfin-ayudante"
name = "Ayudante de Jellyfin"
description = "Busca películas en tu Jellyfin y te dice cómo verlas en la tele."
version = "1.0.0"
permissions = ["app:jellyfin:api"]   # lista cerrada de permisos posibles

[[tools]]
name = "buscar_pelicula"
description = "Busca una película por nombre en Jellyfin"
```

- Permisos: existe una lista cerrada (por ejemplo `fotos:leer`, `apps:instalar`, `app:<id>:api`, `sistema:leer`). Al instalar una habilidad, el admin ve sus permisos en español y los aprueba. Una habilidad nunca obtiene más permisos que los declarados y aprobados.

## 11. Emulador

- Una imagen Docker oficial que levanta Roost entero con un comando, para probarlo en cualquier computadora.
- Un modo demo:
  - fotos de ejemplo libres de derechos
  - métricas simuladas marcadas como "demo"
  - instalaciones simuladas, que no descargan nada
- Después, una imagen para máquina virtual (QEMU o VirtualBox) que arranca directo en Roost.
- El emulador también sirve para los tests de punta a punta.

## 12. Seguridad

- Primer arranque: se crea el admin con una contraseña de 10 caracteres como mínimo.
- Contraseñas: guardadas con `argon2id`.
- Sesiones: cookie `HttpOnly`, `SameSite=Strict` y `Secure` cuando hay HTTPS; vencen a los 30 días.
- Protección CSRF en todo lo que modifica algo.
- Límite de intentos: 5 fallidos por 15 minutos, por IP y por usuario.
- Red: por defecto solo escucha en la red local. El acceso desde afuera de la casa es una opción explícita, con advertencia.
- Docker: el socket da control total de la máquina; solo el módulo `docker` lo usa y solo con acciones del catálogo.
- Validación: todo lo que llega del usuario se valida. Las rutas de archivos se normalizan para impedir salir de la carpeta de datos.
- Logs: sin contraseñas, claves, tokens ni contenido de fotos.
- Privacidad: sin telemetría, nunca.
- Confirmaciones: toda acción destructiva pide confirmación en la interfaz y `confirm: true` en la API.

## 13. Calidad y pruebas

- Antes de cada commit pasan: `cargo fmt --check`, `cargo clippy -- -D warnings` y `cargo test`, y el build del frontend si lo tocaste.
- Tests:
  - unitarios para la lógica (permisos, cola, parseo de `app.toml` y `skill.toml`)
  - de integración para la API con una base temporal
  - Docker se simula detrás de un trait, así los tests no necesitan Docker real
- CI en GitHub Actions con lo mismo, más la compilación para `aarch64` (Raspberry Pi).
- Benchmarks: al final de cada hito, medí la RAM sin hacer nada, la RAM pico, el tamaño del binario y el peso del frontend. Anotalos en `BENCHMARKS.md` con la máquina y el método. Si no lo mediste, escribí "falta medir". Nunca inventes números.

## 14. Reglas que no podés romper

- No copies código, textos, íconos ni diseño de Umbrel. Su licencia (PolyForm Noncommercial) no lo permite.
- Revisá la licencia de todo lo que reutilices. Si no es compatible con MIT o tenés dudas, pará y preguntá.
- Nunca hagas commit de secretos. El token de GitHub está en `GH_TOKEN`: nunca lo muestres, imprimas ni escribas en archivos.
- No inventes números, rankings, reseñas ni datos.
- El README dice que Roost se construye con ayuda de IA y que Franco Senes revisa cada cambio.
- No agregues funciones fuera de este documento sin preguntar.
- Si algo no está claro, preguntá antes de adivinar, sobre todo en lo que no se puede deshacer.

## 15. Cómo trabajar con Git

- Un solo agente, sin varios en paralelo. Commits chicos y en orden; cada uno compila y pasa los tests.
- Mensajes de commit en inglés: `feat:`, `fix:`, `perf:`, `docs:`, `refactor:`, `test:`, `chore:`.
- Una rama por hito (`m0-base`, `m1-docker`…). Al terminar, un Pull Request. Franco revisa y hace el merge: nunca hagas push a `main`.
- La descripción del PR va en español, clara para un programador de 13 años:
  - qué hace este hito
  - por qué se hizo así
  - cómo probarlo a mano, paso a paso
  - números de `BENCHMARKS.md`, antes y después
  - qué quedó pendiente o qué riesgos hay
- Actualizá `PROGRESS.md` al terminar.
- No explores de más: leé solo los archivos que vas a tocar.

## 16. Hitos (en orden, uno por sesión)

Cada hito dice cuándo está listo.

- **M0 — Base**:
  - workspace y perfil release
  - MIT, README, `.gitignore`, `PROGRESS.md`, `BENCHMARKS.md`
  - CI (incluye `aarch64`)
  - *Listo cuando*: el CI pasa y `cargo run` muestra "Roost" en el puerto 8420.
- **M1 — Docker**: módulo `docker` detrás de un trait: listar, iniciar, detener y ver el estado. *Listo cuando*: hay tests con el simulador y una prueba manual con Docker real.
- **M2 — Catálogo**:
  - formato `app.toml` y `docs/apps.md`
  - las 5 apps: Jellyfin, AdGuard Home, Nextcloud, Home Assistant y Vaultwarden, con su licencia revisada
  - *Listo cuando*: las 5 se instalan y abren a mano.
- **M3 — API de apps y eventos**:
  - instalar, iniciar, detener, reiniciar y desinstalar
  - SSE con progreso real (MB descargados) y aviso de RAM insuficiente
  - *Listo cuando*: una instalación muestra su progreso en vivo.
- **M4 — Usuarios y seguridad**:
  - primer arranque, inicio de sesión, roles, suspender
  - límite de intentos y CSRF
  - *Listo cuando*: hay tests de permisos y un miembro no puede instalar apps.
- **M5 — Métricas**: CPU, RAM, disco y temperatura por SSE, solo mientras hay alguien mirando. *Listo cuando*: la CPU de Roost queda en ~0% con el panel cerrado.
- **M6 — Interfaz Material 3**:
  - Inicio, Tienda y Ajustes conectados a la API
  - temas Verde, Azul y Violeta, en claro y oscuro
  - responsive y accesible
  - *Listo cuando*: la primera carga pesa menos de 250 KB comprimida y se usa entera con teclado.
- **M7 — Fotos y videos**:
  - subida retomable, repetidas, miniaturas en cola, EXIF
  - línea de tiempo, visor, álbumes compartidos, papelera, streaming de video
  - *Listo cuando*: subir 1.000 fotos no pasa el límite de RAM ni congela el panel.
- **M8 — Emulador**:
  - imagen Docker y modo demo
  - después, una imagen para máquina virtual
  - *Listo cuando*: `docker run` levanta Roost en modo demo en una máquina limpia.
- **M9 — Agente de IA**:
  - los tres proveedores, claves cifradas, streaming con razonamiento
  - herramientas con aprobación, reglas del admin y aviso de nube
  - pantalla Agente
  - *Listo cuando*: "instalá algo para ver películas" termina en una tarjeta de aprobación y, si se aprueba, en Jellyfin instalado.
- **M10 — Marketplace**:
  - modelos con filtros y "entra en tu RAM"
  - habilidades declarativas con permisos aprobados
  - *Listo cuando*: una habilidad de ejemplo se instala, se usa y se desinstala, y no puede salir de sus permisos.
- **M11 — Pulido de rendimiento**:
  - medir todo en una Raspberry Pi 4 y en una laptop vieja
  - arreglar lo que no cumpla la sección 2
  - `BENCHMARKS.md` final
  - *Listo cuando*: todos los límites de la sección 2 se cumplen con mediciones reales.
