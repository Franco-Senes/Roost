# Roost

Roost convierte una computadora vieja, una mini PC o una Raspberry Pi en el servidor de una familia, sin que nadie tenga que saber Linux.

## Qué incluye Roost

- **Tienda de apps con un clic**: películas (Jellyfin), archivos (Nextcloud), bloqueo de anuncios (AdGuard Home), casa inteligente (Home Assistant), contraseñas (Vaultwarden) y más.
- **Fotos y videos integrados**: una galería propia dentro de Roost para guardar las fotos de la familia en casa, sin pagar por la nube.
- **Agente de IA propio con marketplace**: un asistente que ayuda a usar el servidor, compatible con Ollama (local), OpenRouter y Hack Club AI.
- **Emulador**: para probar Roost entero sin instalarlo en tu máquina.

## Promesas del proyecto

1. **Liviano**: gasta poca RAM y poca CPU.
2. **Rápido**: todo responde al instante.
3. **Hermoso**: interfaz en Material Design 3.
4. **Open source de verdad**: licencia MIT.

## Estado actual

El proyecto está en desarrollo paso a paso. Podés ver qué hitos están listos en [`PROGRESS.md`](PROGRESS.md) y las mediciones reales de rendimiento en [`BENCHMARKS.md`](BENCHMARKS.md).

## Cómo ejecutarlo

Requisitos: Rust estable (`cargo`).

```bash
cargo run
```

Roost arranca por defecto en el puerto `8420` (`http://localhost:8420`). Podés cambiar la dirección con `ROOST_ADDR` (por ejemplo, `ROOST_ADDR=127.0.0.1:8420`).

## Desarrollo asistido por IA

Roost se construye con ayuda de IA y **Franco Senes** revisa cada cambio antes de integrarlo al repositorio.

## Licencia

Distribuido bajo licencia [MIT](LICENSE).
