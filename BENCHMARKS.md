# Benchmarks de Roost

Mediciones reales al final de cada hito. Nunca se inventan números: lo que todavía no se midió figura como `falta medir`.

## Límites objetivo (ver `AGENTS.md` §2)

- **RAM de Roost (sin apps ni Ollama)**: nunca más de 1 GB.
- **CPU sin hacer nada**: cerca de 0%.
- **Interfaz, primera carga**: menos de 250 KB comprimida.
- **Panel visible en Raspberry Pi 4 (red local)**: menos de 1 segundo.

## Registro por hito

| Hito | Máquina | RAM sin hacer nada | RAM pico | CPU sin hacer nada | Tamaño del binario (`release`) | Peso del frontend (comprimido) | Método |
| --- | --- | --- | --- | --- | --- | --- | --- |
| M0 — Base | Intel Core i7-8559U @ 2.70GHz, 16 GB RAM, macOS 15.8.1 (`x86_64`), `rustc 1.100.0` | 1,34 MiB (`1372 KiB` RSS) | 1,92 MiB (`2 015 232 bytes` max RSS) | 0,0% | 1,22 MiB (`1 279 496 bytes`) | no aplica (M6) | Binario `target/release/roost` bajo `/usr/bin/time -l`; reposo medido con `ps -o rss=,pcpu=`; pico tras 100 pedidos `GET /` con `curl`. |
| M1 — Docker | Intel Core i7-8559U @ 2.70GHz, 16 GB RAM, macOS 15.8.1 (`x86_64`), `rustc 1.100.0` | 1,27 MiB (`1296 KiB` RSS) | 1,88 MiB (`1 974 272 bytes` max RSS) | 0,0% | 1,22 MiB (`1 283 584 bytes`) | no aplica (M6) | Binario `target/release/roost` bajo `/usr/bin/time -l`; reposo medido con `ps -o rss=,pcpu=`; pico tras 100 pedidos `GET /` con `curl` (LTO elimina código Docker aún no conectado a la API). |
