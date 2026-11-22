# Progreso de Roost

Seguimiento de los hitos definidos en [`AGENTS.md`](AGENTS.md).

- [x] **M0 — Base**: workspace en Rust, perfil `release`, licencia MIT, `README.md`, `.gitignore`, `PROGRESS.md`, `BENCHMARKS.md`, CI (incluye `aarch64`) y servidor base respondiendo `"Roost"` en el puerto `8420`.
- [x] **M1 — Docker**: módulo `docker` detrás de un trait (listar, iniciar, detener y ver estado).
- [x] **M2 — Catálogo**: formato `app.toml`, `docs/apps.md` y las 5 apps iniciales.
- [x] **M3 — API de apps y eventos**: instalar, iniciar, detener, reiniciar, desinstalar y progreso en vivo por SSE.
- [x] **M4 — Usuarios y seguridad**: primer arranque, sesiones, roles, suspensión, límite de intentos y CSRF.
- [x] **M5 — Métricas**: CPU, RAM, disco y temperatura por SSE solo mientras alguien mira.
- [x] **M6 — Interfaz Material 3**: Inicio, Tienda y Ajustes con temas Verde, Azul y Violeta (claro/oscuro).
- [x] **M7 — Fotos y videos**: subida retomable, duplicados, miniaturas en cola, EXIF, visor, álbumes, papelera y streaming.
- [x] **M8 — Emulador**: imagen Docker, modo demo e imagen de máquina virtual.
- [x] **M9 — Agente de IA**: Ollama, OpenRouter y Hack Club AI, claves cifradas, herramientas con aprobación, pantalla Agente y proyectos locales 24/7 (Antigravity/Claude).
- [x] **M10 — Marketplace**: modelos con filtros y habilidades declarativas con permisos aprobados.
- [x] **M11 — Pulido de rendimiento**: benchmarks finales en macOS (`x86_64`), lanzador 24/7 (`scripts/install-service.sh`) y validación de límites de §2 (medición en Raspberry Pi 4 pendiente de hardware físico).
