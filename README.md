# Open Hippo Campus (PoC)

App de escritorio para crear y gestionar servicios locales en contenedores (Redis, PostgreSQL, MySQL, MongoDB, RabbitMQ, MinIO) con un clic.

Es la **fase 0** del plan descrito en [analisis-orbstack.md](analisis-orbstack.md): la app se conecta a un motor Docker que ya esté corriendo (OrbStack, Docker Desktop, Colima o Rancher Desktop). En una fase posterior levantará su propia VM (Lima en macOS, WSL2 en Windows).

## Stack

| Capa | Tecnología |
|---|---|
| Ventana de escritorio | Tauri 2 (webview del sistema) |
| Interfaz | React 19 + TypeScript + Vite |
| Backend | Rust |
| Docker Engine API | crate `bollard` 0.21 (unix socket en macOS/Linux, named pipe en Windows) |
| Catálogo | Plantillas declarativas en [src-tauri/catalog.json](src-tauri/catalog.json) |

## Requisitos

- Node 20+ y pnpm
- Rust (`rustup`)
- Un motor Docker en ejecución

## Comandos

```bash
pnpm install
```

```bash
pnpm tauri dev
```

```bash
pnpm tauri build
```

Para los tests del backend:

```bash
cd src-tauri && cargo test
```

Para la prueba de humo contra tu Docker real. Crea Redis y PostgreSQL, espera a que estén sanos, se conecta desde el host y los borra:

```bash
cd src-tauri && cargo run --example smoke
```

## Estructura

```
src/                     Frontend React
  App.tsx                Pantallas: servicios, catálogo, contenedores, logs
  api.ts                 Tipos y llamadas a los comandos de Tauri
src-tauri/
  catalog.json           Servicios disponibles (imagen, puertos, env, healthcheck...)
  src/engine.rs          Detecta y conecta con el motor (OrbStack, Docker Desktop...)
  src/catalog.rs         Carga plantillas, rellena {{password}} y {{port.N}}
  src/services.rs        Listar, crear, arrancar, parar, borrar y logs
  src/lib.rs             Comandos expuestos al frontend
```

## Cómo funciona "Crear servicio"

1. Descarga la imagen si no está en local. El progreso se emite con el evento `service-progress`.
2. Elige un nombre libre (`ohc-postgres`, `ohc-postgres-2`...) y puertos del host libres, empezando por el estándar.
3. Genera una contraseña aleatoria y rellena la plantilla.
4. Crea un volumen con nombre (`ohc-postgres-data`) para que los datos persistan.
5. Crea el contenedor con:
   - puertos publicados solo en `127.0.0.1`,
   - healthcheck,
   - `restart: unless-stopped`,
   - etiquetas `ohc.*`, donde se guardan la cadena de conexión y los puertos.
6. Lo arranca. La interfaz muestra "Iniciando" hasta que el healthcheck pasa a "Listo".

## Añadir un servicio al catálogo

Añade una entrada a `src-tauri/catalog.json`:

```json
{
  "id": "mailpit",
  "name": "Mailpit",
  "category": "Correo de pruebas",
  "description": "Servidor SMTP falso con bandeja web.",
  "image": "axllent/mailpit:latest",
  "ports": [
    { "container": 1025, "label": "SMTP" },
    { "container": 8025, "label": "Panel web" }
  ],
  "connection": "smtp://localhost:{{port.1025}}",
  "webUi": "http://localhost:{{port.8025}}"
}
```

## Icono

El original está en [assets/app-icon.png](assets/app-icon.png) (1024×1024, con transparencia y sombra). Si lo cambias, regenera todos los tamaños para macOS y Windows con:

```bash
pnpm tauri icon assets/app-icon.png && rm -rf src-tauri/icons/android src-tauri/icons/ios
```

El logo de la cabecera de la app es [public/app-icon.png](public/app-icon.png), una copia de 128 px.

## Limitaciones conocidas de la PoC

- Necesita un motor Docker ya instalado. Todavía no crea su propia VM.
- La contraseña de cada servicio se guarda en una etiqueta del contenedor, en texto plano. Es aceptable para desarrollo local, pero no para producción.
- La lista se actualiza cada 3 s por sondeo; todavía no usa el stream de eventos de Docker.
