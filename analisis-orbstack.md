# Ingeniería inversa de OrbStack y stack para una app propia de contenedores

**Fecha:** 2026-11-08
**Objeto analizado:** OrbStack 2.2.5 (build 21124), instalado en este Mac (Intel x86_64, `/Applications/OrbStack.app`)
**Método:** documentación pública (orbstack.dev, docs.orbstack.dev/architecture) e inspección estática del bundle con `otool`, `strings`, `file` y lectura de cabeceras. También consulté el motor Docker en ejecución. No se descompiló código.

> Nota: este Mac es Intel. La build para Apple Silicon (arm64) puede variar en algunos detalles, por ejemplo en Rosetta y en el uso de Virtualization.framework.

---

## 1. Resumen

- **OrbStack no "simula" Docker.** Arranca una **VM Linux muy ligera** con un kernel propio. Dentro corre el **Docker Engine oficial** (dockerd 29.8.2, containerd 2.3.6 y runc 1.5.2). El socket de Docker se reenvía a macOS en `~/.orbstack/run/docker.sock`. El `docker` CLI de tu Mac habla con ese socket como si el motor fuera local.
- Su ventaja está en la **capa de virtualización y en la integración**, no en Docker. Tiene un **hipervisor propio escrito en Rust** sobre `Hypervisor.framework`, una **pila de red propia en Go** (gVisor netstack + DNS propio), VirtioFS con caché, memoria dinámica (balloon), un disco *sparse* y una UI nativa en Swift.
- Los contenedores necesitan el kernel de Linux (namespaces y cgroups). Por eso **en Mac y en Windows siempre hace falta una VM Linux**. En Windows lo estándar es **WSL2** (o Hyper-V).

---

## 2. Anatomía del bundle (evidencia)

| Componente | Archivo | Lenguaje / tecnología (evidencia) | Función |
|---|---|---|---|
| **App GUI** | `Contents/MacOS/OrbStack` (38 MB) | **Swift + SwiftUI/AppKit**: enlaza `SwiftUI`, `AppKit` y 25 libs `libswift*`. Usa **gRPC-Swift** (`GRPCCore`, `GRPCNIOTransport`, `NIOHTTP2`), **SwiftProtobuf**, **swift-nio** y **swift-crypto**. Incluye **Sparkle** (actualizaciones), **Sentry** (crashes) y **Defaults** (preferencias). | Ventana principal, menubar, listas de contenedores, imágenes, volúmenes y máquinas |
| **Motor / VM manager** | `Frameworks/OrbStack Helper.app` (79 MB), proceso `OrbStack Helper vmgr` | **Go 1.26.4 + Rust + Swift en un mismo binario**. Enlaza `Hypervisor.framework`, `Virtualization.framework`, `vmnet` e `IOUSBHost`. | Arranca y gestiona la VM, la red, los archivos, el DNS, SSH y Kubernetes |
| **CLI** | `scli.app` → enlaces simbólicos `orb` y `orbctl` | **Go** (go1.26.4) con algo de Swift | `orb create ubuntu`, `orb shell`, `orbctl start`... |
| **Helper privilegiado** | `Library/LaunchServices/dev.orbstack.OrbStack.privhelper` | **Swift**, instalado con `SMPrivilegedExecutables` (SMJobBless) | Tareas de root: `/var/run/docker.sock` y ajustes de red o DNS del sistema |
| **Herramientas cliente** | `MacOS/xbin/` | Binario multi-llamada `docker-tools` (docker, buildx, compose), `kubectl` y `docker-credential-osxkeychain` | CLI estándar de Docker y Kubernetes, ya incluido |
| **Kernel Linux** | `Resources/assets/release/amd64/kernel` | bzImage **Linux 7.2.12-orbstack** (fork propio) | Kernel compartido por todos los contenedores y máquinas |
| **Rootfs** | `rootfs.img` (450 MB) | **EROFS**: FS comprimido y de solo lectura (magic `0xE0F5E1E2`). Base **Alpine**, con docker, containerd, runc, k3s y LXC | Sistema base inmutable de la VM |
| **Initramfs** | `rpack` | Archivo **cpio** | Arranque temprano |
| **Datos** | `~/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw` | Imagen raw **sparse**: tamaño lógico de unos 500 GB, pero solo **719 MB en disco** | Imágenes, contenedores, volúmenes y máquinas |
| **Swap** | `swap.img` | 1 GB | Swap de la VM |

### 2.1 Librerías Go detectadas en `vmgr` (backend)

- `github.com/orbstack/macvirt/vmgr` y `.../scon`: paquetes propios. `vmgr` es el gestor de la VM y `scon` el gestor de "máquinas" y contenedores de sistema.
- **`gvisor.dev/gvisor`**: la pila TCP/IP en espacio de usuario de Google. Es la base de su **red virtual propia** (NAT, IPv4/IPv6 y port-forwarding).
- **`github.com/miekg/dns`**: el **servidor DNS propio** que resuelve `*.orb.local` y reenvía el resto al DNS de macOS, para que funcione con VPNs.
- **`github.com/gliderlabs/ssh`**: servidor SSH para `ssh orb` y para editar en remoto con VS Code.
- **`github.com/buildbarn/go-xdr`**: XDR es el formato de **NFS**. Existe `nfs.sock`, así que lo más probable es que expongan `~/OrbStack` (volúmenes e imágenes) a Finder vía NFS.
- **`k8s.io/client-go`, `apimachinery`** y **`moby/buildkit`**: integración con Kubernetes (k3s dentro de la VM) y con builds.
- **`github.com/diskfs/go-diskfs`**: manejo de las imágenes de disco.
- **`creachadair/jrpc2`** (JSON-RPC), `fsnotify` (fork propio), `go-keychain`, `sentry-go` y `logrus`.

### 2.2 Hipervisor propio en Rust (lo más diferencial)

Módulos Rust propios que aparecen en las rutas de compilación: **`devices`, `hvf`, `vmm`, `arch`, `cpuid`, `utils`, `libvirtue`, `brnet`, `gruel`, `memmage`**.

- La estructura `devices/ hvf/ vmm/ arch/ cpuid/` es la misma de **libkrun**, que a su vez deriva de **Firecracker**. A eso se suma el crate **`vmm-sys-util`** de **rust-vmm**. Todo indica un **VMM propio, basado en ese linaje o inspirado en él**, que corre directamente sobre **`Hypervisor.framework`** de Apple (el módulo `hvf`).
- Dispositivos virtio que se ven en el binario: **virtio-fs, virtio-net, virtio-vsock, virtio-balloon, virtio-snd, virtio-console y virtio-mmio**.
- `iced-x86`: un decodificador de instrucciones x86, típico de un VMM que debe emular accesos MMIO.
- `tokio`, `axum`, `hyper` y `tower`: servicios asíncronos y HTTP internos.
- `cpal` y `coreaudio-rs`: audio para las máquinas Linux. `objc2-io-usb-host`: **USB passthrough**.
- Además enlaza `Virtualization.framework` (VZ). Es probable que lo use para funciones que Apple solo expone ahí, como **Rosetta para Linux** en Apple Silicon, o como alternativa.

### 2.3 Dentro de la VM (consultado en vivo)

```
Server: Docker Engine 29.8.2 | containerd v2.3.6 | runc 1.5.2 | docker-init 0.19.0
Storage driver: overlayfs | cgroup v2 | OS: "OrbStack"
Kernel: 7.2.12-orbstack-00394-gbecdd6130068
```

- **Un solo kernel compartido** para Docker y para las "máquinas" Linux (16 distros), al estilo de WSL2. Las máquinas son contenedores de sistema (tipo LXC) gestionados por `scon`.
- Kubernetes con **k3s**.

### 2.4 Canales de comunicación host ↔ VM (`~/.orbstack/run/`)

| Socket | Uso |
|---|---|
| `docker.sock` | API de Docker Engine reenviada desde la VM |
| `vmcontrol.sock` | Control de la VM (arrancar, parar, ajustes) desde la GUI o la CLI |
| `sconrpc.sock`, `sconrpc2.sock` | RPC con el gestor de máquinas `scon` (gRPC/JSON-RPC) |
| `sconssh.sock`, `sconssh-public.sock` | SSH hacia las máquinas |
| `nfs.sock` | Servidor NFS para exponer los archivos de la VM en el Mac |

El transporte host ↔ VM es **virtio-vsock** (más de 360 referencias a `vsock` en el binario).

### 2.5 Diagrama de arquitectura

```
┌───────────────────────────── macOS ─────────────────────────────┐
│  OrbStack.app (Swift/SwiftUI)     orb / orbctl (Go)   docker CLI │
│        │ gRPC / JSON-RPC               │                  │      │
│        ▼                               ▼                  ▼      │
│  ~/.orbstack/run/{vmcontrol,sconrpc}.sock          docker.sock   │
│        │                                                  │      │
│  ┌─────┴──────────── OrbStack Helper "vmgr" ──────────────┴───┐  │
│  │ Go: vmgr · scon client · red gVisor · DNS · SSH · NFS · k8s │  │
│  │ Rust: VMM propio (hvf, devices virtio, balloon, vsock)      │  │
│  │ Swift: puente con frameworks de Apple                       │  │
│  └──────────── Hypervisor.framework / Virtualization.fw ───────┘  │
│                         │ virtio (fs, net, vsock, balloon)        │
│  ┌──────────────────────▼──────── VM Linux ──────────────────┐   │
│  │ Kernel 7.2.12-orbstack · rootfs EROFS (Alpine)            │   │
│  │ dockerd + containerd + runc  │  scon (máquinas LXC)  │ k3s │   │
│  │ data.img (sparse) → imágenes, volúmenes, contenedores     │   │
│  └───────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

### 2.6 Por qué es rápido y ligero

1. **VMM a medida** en lugar de QEMU: arranca en segundos y usa menos del 0,1 % de CPU en reposo.
2. **Memoria dinámica** (virtio-balloon): devuelve RAM a macOS cuando no la usa.
3. **VirtioFS con caché dinámica** para los bind mounts.
4. **Red propia**: port-forwarding por eventos, `localhost` automático, dominios `*.orb.local`, IPv6 y compatibilidad con VPN.
5. **Disco sparse y rootfs EROFS**: la instalación ocupa unos 10 MB de datos y el disco crece y se encoge solo.
6. **Rosetta** en lugar de QEMU para las imágenes x86 en Apple Silicon.
7. **UI nativa** en Swift en lugar de Electron.

---

## 3. Stack recomendado para tu app (Mac + Windows)

### 3.1 Principio clave

**No reimplementes Docker ni un hipervisor al principio.** Ejecuta un motor real (dockerd/containerd o Podman) dentro de una VM Linux gestionada por tu app. Tu valor está en la **UI, el catálogo de servicios** (Redis, Postgres...) y la integración.

### 3.2 Opción A: recomendada para el MVP (multiplataforma)

| Capa | macOS | Windows | Por qué |
|---|---|---|---|
| **Virtualización** | **Lima** (driver `vz`, que usa Virtualization.framework y virtiofs) o el framework **Apple Containerization** (Swift, open source, macOS 26 o superior) | **WSL2**: importar una distro propia con `wsl --import` | Es lo que usan Rancher Desktop, Colima y Docker Desktop. Probado y gratuito |
| **Motor de contenedores** | dockerd + containerd (Moby) o Podman dentro de la VM | Igual, dentro de la distro WSL2 | Compatibilidad total con `docker`, Compose e imágenes de Docker Hub |
| **Distro de la VM** | Alpine o Debian mínima (ideal: rootfs de solo lectura + disco de datos aparte, como OrbStack) | Igual | Arranque rápido y actualizaciones atómicas |
| **Red host ↔ VM** | Lima: port-forward automático. Opcional: `gvisor-tap-vsock` | WSL2 reenvía `localhost` de forma nativa (modo *mirrored*) | Redis en `localhost:6379` sin configurar nada |
| **Backend / daemon** | **Go** (el ecosistema de contenedores está en Go: moby, containerd, lima, gvisor) **o Rust** | Mismo binario compilado para Windows | Un solo código para ambas plataformas |
| **API con el motor** | Docker Engine API sobre unix socket (`/var/run/docker.sock` reenviado) | Named pipe `npipe:////./pipe/docker_engine` o socket TCP de WSL | SDK oficial `github.com/docker/docker/client` (Go) o crate `bollard` (Rust) |
| **UI de escritorio** | **Tauri 2** (Rust + webview del sistema) con React/TypeScript | Igual, con WebView2 | Binario de unos 10 MB frente a más de 150 MB con Electron. Si eliges Rust, backend y UI comparten lenguaje |
| **Alternativa de UI** | Electron | Electron | Lo usan Docker Desktop, Podman Desktop y Rancher Desktop. Más pesado, pero muy maduro |
| **Logs / terminal en la UI** | `xterm.js` + streaming de la API (`/containers/{id}/logs`, `/exec`) | Igual | Consola integrada |
| **Actualizaciones** | Sparkle o el updater de Tauri | Updater de Tauri o MSIX | |
| **Crashes / telemetría** | Sentry | Sentry | Igual que OrbStack |
| **Helper privilegiado** | `SMAppService` (macOS 13 o superior) | Servicio de Windows (opcional) | Solo si necesitas `/var/run/docker.sock` o tocar el DNS del sistema |

### 3.3 Opción B: estilo OrbStack (nativo, máximo rendimiento y alto coste)

- **UI nativa por plataforma:** Swift/SwiftUI en macOS y **WinUI 3 / C#** en Windows, con un núcleo común en Go o Rust expuesto por gRPC.
- **VMM propio:**
  - macOS: Rust sobre `Hypervisor.framework` (punto de partida: **libkrun**/**krunkit**, rust-vmm, Firecracker) o Swift sobre `Virtualization.framework` (más sencillo; referencia: **vfkit**).
  - Windows: **Hyper-V HCS** (Host Compute Service, la base de WSL2) o **WHP** (Windows Hypervisor Platform).
- **Red propia:** gVisor netstack + servidor DNS (`miekg/dns`) para dominios `*.tuapp.local`.
- **Kernel propio** con parches, rootfs EROFS y disco sparse.
- Requiere de 1 a 2 años de trabajo de un equipo con experiencia en virtualización. Solo tiene sentido después de validar el producto con la Opción A.

### 3.4 Proyectos de referencia open source

| Proyecto | Qué aprender |
|---|---|
| **Lima** / **Colima** (Go) | Gestión de VMs en macOS, `vz`, virtiofs, port-forward |
| **Rancher Desktop** (Electron + Lima/WSL2) | La misma arquitectura multiplataforma de la Opción A |
| **Podman Desktop** (Electron + TS) | UI de contenedores, extensiones, catálogo |
| **Apple `container` / Containerization** (Swift) | Una VM ligera por contenedor en macOS |
| **libkrun / krunkit** (Rust) | VMM mínimo sobre HVF, la misma línea que OrbStack |
| **vfkit** (Go) | Uso directo de Virtualization.framework |
| **gvisor-tap-vsock** (Go) | Red virtual en espacio de usuario, como la de OrbStack |

---

## 4. Módulo "Catálogo de servicios" (Redis, Postgres, etc.)

El catálogo es una **plantilla declarativa** que tu app convierte en llamadas a la Docker API o en un `compose.yaml`.

```yaml
# catalog/postgres.yaml
id: postgres
name: PostgreSQL
image: postgres:17-alpine
ports: [{ container: 5432, host: 5432 }]
env:
  POSTGRES_USER: dev
  POSTGRES_PASSWORD: "{{generated}}"
  POSTGRES_DB: app
volumes: [{ name: "{{id}}-data", path: /var/lib/postgresql/data }]
healthcheck: ["CMD", "pg_isready", "-U", "dev"]
connection: "postgres://dev:{{POSTGRES_PASSWORD}}@localhost:5432/app"
```

| Servicio | Imagen | Puerto | Variables mínimas |
|---|---|---|---|
| Redis | `redis:8-alpine` | 6379 | — |
| PostgreSQL | `postgres:17-alpine` | 5432 | `POSTGRES_PASSWORD` |
| MySQL | `mysql:8.4` | 3306 | `MYSQL_ROOT_PASSWORD` |
| MongoDB | `mongo:8` | 27017 | `MONGO_INITDB_ROOT_USERNAME/PASSWORD` |
| RabbitMQ | `rabbitmq:4-management` | 5672 / 15672 | — |
| MinIO (S3) | `minio/minio` | 9000 / 9001 | `MINIO_ROOT_USER/PASSWORD`, cmd `server /data` |
| Kafka | `apache/kafka` | 9092 | modo KRaft |
| Mailpit | `axllent/mailpit` | 1025 / 8025 | — |

> Comprueba las etiquetas de imagen vigentes en Docker Hub antes de fijarlas.

**Flujo al pulsar "Crear Postgres":**
1. `POST /images/create?fromImage=postgres&tag=17-alpine` (pull con progreso en streaming)
2. `POST /volumes/create` con nombre `postgres-data`
3. `POST /containers/create` con `Env`, `HostConfig.PortBindings`, `Mounts`, `Healthcheck` y labels `tuapp.service=postgres`
4. `POST /containers/{id}/start`
5. Esperar a que `/containers/{id}/json` devuelva `State.Health.Status == healthy` y mostrar la cadena de conexión con un botón "Copiar"
6. Para stacks de varios servicios: generar un `compose.yaml` y lanzar `docker compose up -d`

**Funciones de UI que vale la pena copiar de OrbStack:** lista de contenedores agrupada por proyecto de Compose, logs en vivo, terminal (`exec`), explorador de archivos de volúmenes, botón "abrir en navegador" y dominios locales (`postgres.tuapp.local`, con DNS propio y proxy inverso).

---

## 5. Hoja de ruta sugerida

| Fase | Entregable | Tecnologías |
|---|---|---|
| **0. Prueba de concepto** (1–2 semanas) | App Tauri que lista y arranca contenedores contra un Docker existente | Tauri + React + `bollard` |
| **1. Motor propio** (3–6 semanas) | La app crea su propia VM: Lima en macOS y distro WSL2 en Windows, con dockerd dentro | Lima, `wsl --import`, rootfs Alpine |
| **2. Catálogo** (2–4 semanas) | Redis, Postgres, MySQL, Mongo... con un clic, volúmenes persistentes y cadenas de conexión | Plantillas YAML + Docker API |
| **3. Experiencia de desarrollo** | Logs, terminal, Compose, dominios `*.local`, menubar o tray | xterm.js, DNS propio |
| **4. Optimización** | Memoria dinámica, rootfs de solo lectura, disco sparse, arranque menor a 3 s | Kernel ajustado, EROFS, balloon |
| **5. (Opcional) VMM propio** | Reemplazar Lima por un VMM a medida | Rust + HVF / Hyper-V HCS |

---

## 6. Consideraciones legales

OrbStack es software propietario (Orbital Labs, LLC). Este análisis describe su arquitectura para fines de interoperabilidad y diseño. No copies su código, binarios, kernel, marca ni diseño visual. Todo lo recomendado arriba tiene alternativas open source con licencias permisivas: Apache-2.0 (Moby, containerd, Lima, gVisor, libkrun) y MIT/Apache (Tauri).
