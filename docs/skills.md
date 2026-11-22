# Formato de habilidades del Agente (`skills/<id>/skill.toml`)

Las habilidades suman herramientas declarativas al agente de IA de Roost sin ejecutar código arbitrario.

## Ejemplo

```toml
id = "jellyfin-ayudante"
name = "Ayudante de Jellyfin"
description = "Busca películas en tu Jellyfin y te dice cómo verlas en la tele."
version = "1.0.0"
permissions = ["app:jellyfin:api"]

[[tools]]
name = "buscar_pelicula"
description = "Busca una película o serie por nombre en tu Jellyfin"
```

## Lista cerrada de permisos

Una habilidad nunca puede obtener más permisos que los declarados en `skill.toml` y aprobados explícitamente por el administrador al instalarla:

| Permiso | Explicación en español para el admin |
| --- | --- |
| `sistema:leer` | Ver el estado del equipo (procesador, memoria y espacio) |
| `fotos:leer` | Buscar en tus fotos y álbumes propios |
| `apps:instalar` | Proponer instalar apps de la Tienda |
| `app:<id>:api` | Hablar con la app `<id>` instalada en tu casa |
