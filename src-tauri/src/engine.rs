//! Conexión con el motor de contenedores (Docker Engine API).
//!
//! La app no ejecuta Docker por sí misma: se conecta al socket de un motor
//! que ya esté corriendo (OrbStack, Docker Desktop, Colima, Rancher Desktop...).
//! En una fase posterior la app levantará su propia VM (Lima / WSL2).

use std::sync::Mutex;

use bollard::{API_DEFAULT_VERSION, Docker};
use serde::Serialize;

/// Segundos de espera para cada petición a la API.
const TIMEOUT_SECS: u64 = 120;

#[derive(Clone)]
pub struct Connection {
    pub docker: Docker,
    pub endpoint: String,
}

#[derive(Default)]
pub struct Engine {
    current: Mutex<Option<Connection>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub connected: bool,
    pub endpoint: Option<String>,
    pub provider: Option<String>,
    pub server_version: Option<String>,
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
    pub cpus: Option<i64>,
    pub memory_bytes: Option<i64>,
    pub containers: Option<i64>,
    pub running: Option<i64>,
    pub images: Option<i64>,
    pub error: Option<String>,
    pub tried: Vec<String>,
}

/// Rutas donde suelen escuchar los motores más comunes, en orden de preferencia.
fn candidate_endpoints() -> Vec<String> {
    let mut list = Vec::new();

    if let Ok(host) = std::env::var("DOCKER_HOST") {
        let path = host
            .trim_start_matches("unix://")
            .trim_start_matches("npipe://")
            .to_string();
        if !path.is_empty() {
            list.push(path);
        }
    }

    #[cfg(unix)]
    {
        list.push("/var/run/docker.sock".into());
        if let Ok(home) = std::env::var("HOME") {
            for rel in [
                ".orbstack/run/docker.sock",      // OrbStack
                ".docker/run/docker.sock",        // Docker Desktop
                ".colima/default/docker.sock",    // Colima
                ".rd/docker.sock",                // Rancher Desktop
            ] {
                list.push(format!("{home}/{rel}"));
            }
        }
    }

    #[cfg(windows)]
    {
        list.push("//./pipe/docker_engine".into()); // Docker Desktop / WSL2
        list.push("//./pipe/podman-machine-default".into());
    }

    list.dedup();
    list
}

fn endpoint_exists(path: &str) -> bool {
    #[cfg(unix)]
    {
        std::path::Path::new(path).exists()
    }
    #[cfg(windows)]
    {
        // Los named pipes no se pueden comprobar con Path::exists.
        let _ = path;
        true
    }
}

/// Deduce qué producto hay detrás del socket, para mostrarlo en la UI.
fn guess_provider(endpoint: &str, os: Option<&str>) -> String {
    let real = std::fs::canonicalize(endpoint)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| endpoint.to_string());
    let hay = format!("{real} {}", os.unwrap_or_default()).to_lowercase();
    let name = if hay.contains("orbstack") {
        "OrbStack"
    } else if hay.contains("docker desktop") || hay.contains("/.docker/run") {
        "Docker Desktop"
    } else if hay.contains("colima") {
        "Colima"
    } else if hay.contains("/.rd/") || hay.contains("rancher") {
        "Rancher Desktop"
    } else if hay.contains("podman") {
        "Podman"
    } else {
        "Docker Engine"
    };
    name.to_string()
}

impl Engine {
    /// Devuelve la conexión activa o intenta conectar con el primer motor que responda.
    pub async fn connection(&self) -> Result<Connection, String> {
        if let Some(conn) = self.current.lock().unwrap().clone() {
            return Ok(conn);
        }
        let (conn, _) = self.connect().await;
        conn.ok_or_else(|| "No se encontró ningún motor de contenedores en ejecución".to_string())
    }

    /// Olvida la conexión actual (por ejemplo, si el motor se apagó).
    pub fn reset(&self) {
        *self.current.lock().unwrap() = None;
    }

    async fn connect(&self) -> (Option<Connection>, Vec<String>) {
        let mut tried = Vec::new();
        for endpoint in candidate_endpoints() {
            if !endpoint_exists(&endpoint) {
                continue;
            }
            tried.push(endpoint.clone());
            let Ok(docker) = Docker::connect_with_socket(&endpoint, TIMEOUT_SECS, API_DEFAULT_VERSION)
            else {
                continue;
            };
            if docker.ping().await.is_ok() {
                let conn = Connection { docker, endpoint };
                *self.current.lock().unwrap() = Some(conn.clone());
                return (Some(conn), tried);
            }
        }
        (None, tried)
    }

    pub async fn status(&self) -> EngineStatus {
        // Si la conexión guardada dejó de responder, se vuelve a buscar.
        let saved = self.current.lock().unwrap().clone();
        let mut tried = Vec::new();
        let conn = match saved {
            Some(c) if c.docker.ping().await.is_ok() => Some(c),
            _ => {
                self.reset();
                let (c, t) = self.connect().await;
                tried = t;
                c
            }
        };

        let Some(conn) = conn else {
            return EngineStatus {
                connected: false,
                endpoint: None,
                provider: None,
                server_version: None,
                os: None,
                kernel: None,
                arch: None,
                cpus: None,
                memory_bytes: None,
                containers: None,
                running: None,
                images: None,
                error: Some(
                    "No hay ningún motor de contenedores en ejecución. Abre OrbStack, Docker Desktop o Colima."
                        .into(),
                ),
                tried,
            };
        };

        let version = conn.docker.version().await.ok();
        let info = conn.docker.info().await.ok();
        let os = info.as_ref().and_then(|i| i.operating_system.clone());

        EngineStatus {
            connected: true,
            provider: Some(guess_provider(&conn.endpoint, os.as_deref())),
            endpoint: Some(conn.endpoint),
            server_version: version.as_ref().and_then(|v| v.version.clone()),
            os,
            kernel: info.as_ref().and_then(|i| i.kernel_version.clone()),
            arch: info.as_ref().and_then(|i| i.architecture.clone()),
            cpus: info.as_ref().and_then(|i| i.ncpu),
            memory_bytes: info.as_ref().and_then(|i| i.mem_total),
            containers: info.as_ref().and_then(|i| i.containers),
            running: info.as_ref().and_then(|i| i.containers_running),
            images: info.as_ref().and_then(|i| i.images),
            error: None,
            tried,
        }
    }
}
