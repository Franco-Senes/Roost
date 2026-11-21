# Fotos y videos en Roost

Roost incluye una galería familiar privada integrada, sin depender de la nube ni instalar apps aparte.

## Cómo se guardan tus archivos

- **Originales intocables**: se guardan tal cual los subiste en `photos/originals/<usuario>/<año>/<mes>/`. Nunca se recomprimen ni se modifican.
- **Sin duplicados**: cada archivo se identifica con un hash `blake3`; si subís la misma foto dos veces, Roost la detecta y no ocupa espacio doble.
- **Miniaturas en segundo plano**: Roost genera miniaturas WebP de `256 px` y `1024 px` en `photos/thumbs/` usando una cola persistente de baja prioridad que se pausa si la CPU supera el 80% y retoma donde quedó ante un corte de luz.
- **Privacidad estricta**: cada persona ve únicamente sus propias fotos y los álbumes que alguien compartió explícitamente con ella. Ni siquiera el administrador puede ver las fotos privadas de otros miembros.
- **Papelera**: las fotos borradas quedan en la papelera por 30 días antes de eliminarse definitivamente.

## Limitación honesta sobre la copia automática en segundo plano (PWA)

Roost funciona en el navegador y se puede instalar como PWA en el celular, pero **los navegadores móviles (especialmente Safari en iPhone/iOS) restringen la ejecución en segundo plano**. Por eso Roost **no promete copia de seguridad automática invisible con el teléfono bloqueado**: para subir tus fotos, abrí Roost y tocá **Subir** (la subida es por partes y retomable si se corta el Wi-Fi).
