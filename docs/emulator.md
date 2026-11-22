# Emulador de Roost (Docker y Máquina Virtual)

El emulador permite probar Roost entero en cualquier computadora sin tocar tu sistema ni descargar apps reales.

## Qué hace el modo demo (`ROOST_DEMO=1`)

- **Fotos de ejemplo libres de derechos**: al crear la cuenta inicial en modo demo, Roost genera 3 imágenes originales de ejemplo y un álbum **"Recuerdos de prueba"** sin ocupar espacio en el repositorio.
- **Métricas simuladas**: CPU, RAM, disco y temperatura se muestran con la etiqueta **Modo demo** (`"demo": true`).
- **Instalaciones simuladas**: instalar, iniciar, detener, reiniciar y desinstalar apps emite progreso real en MB por SSE sin descargar contenedores reales.

## Probarlo con Docker (un solo comando)

```bash
docker compose -f docker/compose.demo.yaml up --build
```

O compilando y corriendo la imagen directamente:

```bash
docker build -f docker/Dockerfile -t roost:demo .
docker run --rm -p 8420:8420 -e ROOST_DEMO=1 roost:demo
```

Luego abrí `http://localhost:8420` en tu navegador.

## Máquina virtual (QEMU / VirtualBox)

El archivo `docker/vm/cloud-init.yaml` permite arrancar una imagen cloud de Debian 12 en QEMU o VirtualBox con el servicio `roost.service` iniciado automáticamente en el puerto `8420`.

## Licencia de `ffmpeg` en la imagen

La imagen de Docker instala el paquete oficial `ffmpeg` de Debian (`debian:bookworm-slim`), distribuido bajo **LGPL-2.1+ / GPL-2.0+** según los códecs compilados por Debian. Roost **no enlaza** las librerías de `ffmpeg`: lo ejecuta únicamente como proceso externo independiente cuando hace falta generar miniaturas o convertir videos, cumpliendo íntegramente con la licencia de Debian/FFmpeg y manteniendo el binario de Roost bajo licencia **MIT**.
