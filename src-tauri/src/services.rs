//! Contenedores: listar, crear servicios del catálogo, arrancar, parar, borrar y logs.

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, ContainerSummary, HealthConfig, HostConfig, PortBinding, RestartPolicy,
    RestartPolicyNameEnum, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, ListContainersOptionsBuilder,
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, StopContainerOptionsBuilder,
};
use futures_util::StreamExt;
use serde::Serialize;

use crate::catalog::{self, CatalogService, Values};

/// Prefijo de las etiquetas que la app pone en sus contenedores.
const L_MANAGED: &str = "ohc.managed";
const L_SERVICE: &str = "ohc.service";
const L_CONNECTION: &str = "ohc.connection";
const L_WEB_UI: &str = "ohc.web_ui";
const L_PORTS: &str = "ohc.ports";
const L_VOLUME: &str = "ohc.volume";

/// Nombre del evento que informa del progreso al crear un servicio.
pub const PROGRESS_EVENT: &str = "service-progress";

/// Función que recibe el progreso (en la app, lo emite como evento de Tauri).
pub type Reporter<'a> = &'a (dyn Fn(Progress) + Send + Sync);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PortView {
    pub private_port: u16,
    pub public_port: Option<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerView {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub health: Option<String>,
    pub ports: Vec<PortView>,
    pub created: i64,
    pub managed: bool,
    pub service_id: Option<String>,
    pub service_name: Option<String>,
    pub connection: Option<String>,
    pub web_ui: Option<String>,
    pub compose_project: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub service_id: String,
    /// pull | create | start | done | error
    pub phase: String,
    pub message: String,
    pub percent: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedService {
    pub id: String,
    pub name: String,
    pub connection: String,
    pub web_ui: Option<String>,
}

/// Convierte un enum de bollard a su texto de la API ("running", "healthy"...).
fn enum_text<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn to_view(c: ContainerSummary) -> ContainerView {
    let labels = c.labels.unwrap_or_default();
    let service_id = labels.get(L_SERVICE).cloned();
    let service_name = service_id
        .as_deref()
        .and_then(catalog::find)
        .map(|s| s.name);

    // Docker devuelve cada puerto dos veces (IPv4 e IPv6): se quitan duplicados.
    let mut seen = HashSet::new();
    let mut ports: Vec<PortView> = c
        .ports
        .unwrap_or_default()
        .into_iter()
        .filter(|p| seen.insert((p.private_port, p.public_port)))
        .map(|p| PortView { private_port: p.private_port, public_port: p.public_port })
        .collect();
    ports.sort_by_key(|p| (p.private_port, p.public_port));

    ContainerView {
        id: c.id.unwrap_or_default(),
        name: c
            .names
            .and_then(|n| n.into_iter().next())
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image: c.image.unwrap_or_default(),
        state: c.state.as_ref().map(enum_text).unwrap_or_default(),
        status: c.status.unwrap_or_default(),
        health: c
            .health
            .and_then(|h| h.status)
            .map(|s| enum_text(&s))
            .filter(|s| !s.is_empty() && s != "none"),
        ports,
        created: c.created.unwrap_or_default(),
        managed: labels.get(L_MANAGED).is_some_and(|v| v == "true"),
        service_id,
        service_name,
        connection: labels.get(L_CONNECTION).cloned(),
        web_ui: labels.get(L_WEB_UI).cloned(),
        compose_project: labels.get("com.docker.compose.project").cloned(),
    }
}

pub async fn list(docker: &Docker) -> Result<Vec<ContainerView>, String> {
    let opts = ListContainersOptionsBuilder::new().all(true).build();
    let mut items: Vec<ContainerView> = docker
        .list_containers(Some(opts))
        .await
        .map_err(|e| format!("No se pudo listar contenedores: {e}"))?
        .into_iter()
        .map(to_view)
        .collect();
    // Primero los gestionados por la app, luego los más recientes.
    items.sort_by(|a, b| b.managed.cmp(&a.managed).then(b.created.cmp(&a.created)));
    Ok(items)
}

pub async fn start(docker: &Docker, id: &str) -> Result<(), String> {
    docker
        .start_container(id, None)
        .await
        .map_err(|e| format!("No se pudo arrancar: {e}"))
}

pub async fn stop(docker: &Docker, id: &str) -> Result<(), String> {
    let opts = StopContainerOptionsBuilder::new().t(10).build();
    docker
        .stop_container(id, Some(opts))
        .await
        .map_err(|e| format!("No se pudo detener: {e}"))
}

/// Borra el contenedor. Con `delete_data` también borra el volumen de datos
/// que la app creó para él.
pub async fn remove(docker: &Docker, id: &str, delete_data: bool) -> Result<(), String> {
    let volume = docker
        .inspect_container(id, None)
        .await
        .ok()
        .and_then(|c| c.config)
        .and_then(|c| c.labels)
        .and_then(|l| l.get(L_VOLUME).cloned());

    let opts = RemoveContainerOptionsBuilder::new().force(true).build();
    docker
        .remove_container(id, Some(opts))
        .await
        .map_err(|e| format!("No se pudo borrar el contenedor: {e}"))?;

    if delete_data {
        if let Some(volume) = volume {
            docker
                .remove_volume(&volume, None::<bollard::query_parameters::RemoveVolumeOptions>)
                .await
                .map_err(|e| format!("Contenedor borrado, pero no el volumen {volume}: {e}"))?;
        }
    }
    Ok(())
}

pub async fn logs(docker: &Docker, id: &str, tail: u32) -> Result<String, String> {
    let opts = LogsOptionsBuilder::new()
        .stdout(true)
        .stderr(true)
        .tail(&tail.to_string())
        .build();
    let mut stream = docker.logs(id, Some(opts));
    let mut out = String::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(line) => out.push_str(&line.to_string()),
            Err(e) => return Err(format!("No se pudieron leer los logs: {e}")),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Creación de servicios del catálogo
// ---------------------------------------------------------------------------

/// Comprueba el puerto en el Mac/PC. Se prueban las dos direcciones porque
/// en macOS un bind a 127.0.0.1 puede funcionar aunque otro proceso tenga 0.0.0.0.
fn port_is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok() && TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Puertos del host que ya usan otros contenedores. El motor los reserva por
/// su cuenta y no siempre se ven desde el host, así que hay que leerlos de Docker:
///   - los publicados por cualquier contenedor en marcha;
///   - los reservados en etiquetas por contenedores de la app, aunque estén parados.
fn reserved_ports(existing: &[ContainerSummary]) -> HashSet<u16> {
    let published = existing
        .iter()
        .flat_map(|c| c.ports.iter().flatten())
        .filter_map(|p| p.public_port);
    let labelled = existing
        .iter()
        .filter_map(|c| c.labels.as_ref()?.get(L_PORTS))
        .flat_map(|s| s.split(',').filter_map(|p| p.split('=').nth(1)?.parse().ok()));
    published.chain(labelled).collect()
}

/// Busca un puerto libre empezando por el puerto estándar del servicio.
fn pick_port(preferred: u16, taken: &mut HashSet<u16>) -> Result<u16, String> {
    let mut port = preferred;
    for _ in 0..200 {
        if !taken.contains(&port) && port_is_free(port) {
            taken.insert(port);
            return Ok(port);
        }
        port = port.checked_add(1).ok_or("Sin puertos libres")?;
    }
    Err(format!("No hay puertos libres cerca de {preferred}"))
}

/// Elige un nombre libre. También evita nombres cuyo volumen `<nombre>-data`
/// siga existiendo: reutilizarlo cargaría datos viejos con otra contraseña.
fn pick_name(service_id: &str, existing: &[ContainerSummary], volumes: &HashSet<String>) -> String {
    let used: HashSet<String> = existing
        .iter()
        .flat_map(|c| c.names.clone().unwrap_or_default())
        .map(|n| n.trim_start_matches('/').to_string())
        .collect();
    let free = |n: &String| !used.contains(n) && !volumes.contains(&format!("{n}-data"));
    let base = format!("ohc-{service_id}");
    if free(&base) {
        return base;
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(free)
        .expect("rango infinito")
}

fn emit(report: Reporter<'_>, service_id: &str, phase: &str, message: impl Into<String>, percent: Option<f64>) {
    report(Progress {
        service_id: service_id.to_string(),
        phase: phase.to_string(),
        message: message.into(),
        percent,
    });
}

/// Descarga la imagen si no está ya en local, informando del progreso.
async fn ensure_image(report: Reporter<'_>, docker: &Docker, svc: &CatalogService) -> Result<(), String> {
    if docker.inspect_image(&svc.image).await.is_ok() {
        emit(report, &svc.id, "pull", format!("Imagen {} ya disponible", svc.image), Some(100.0));
        return Ok(());
    }

    emit(report, &svc.id, "pull", format!("Descargando {}…", svc.image), Some(0.0));
    let opts = CreateImageOptionsBuilder::new().from_image(&svc.image).build();
    let mut stream = docker.create_image(Some(opts), None, None);

    // Progreso por capa: id -> (bytes descargados, bytes totales)
    let mut layers: HashMap<String, (i64, i64)> = HashMap::new();
    let mut last_percent = -1.0;
    while let Some(item) = stream.next().await {
        let info = item.map_err(|e| format!("Error al descargar {}: {e}", svc.image))?;
        if let Some(err) = info.error_detail.and_then(|d| d.message) {
            return Err(format!("Error al descargar {}: {err}", svc.image));
        }
        let status = info.status.unwrap_or_default();
        if let (Some(id), Some(detail)) = (info.id, info.progress_detail) {
            if let (Some(cur), Some(total)) = (detail.current, detail.total) {
                if status.starts_with("Downloading") && total > 0 {
                    layers.insert(id, (cur, total));
                }
            } else if status.starts_with("Download complete") || status.starts_with("Pull complete") {
                if let Some(entry) = layers.get_mut(&id) {
                    entry.0 = entry.1;
                }
            }
        }
        let (cur, total) = layers.values().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        if total > 0 {
            let percent = (cur as f64 / total as f64 * 100.0).min(100.0);
            // Limita los eventos a cambios de al menos 1 %.
            if percent - last_percent >= 1.0 {
                last_percent = percent;
                let mb = |b: i64| b as f64 / 1_048_576.0;
                emit(
                    report,
                    &svc.id,
                    "pull",
                    format!("Descargando {} · {:.1} / {:.1} MB", svc.image, mb(cur), mb(total)),
                    Some(percent),
                );
            }
        }
    }
    Ok(())
}

pub async fn create(report: Reporter<'_>, docker: &Docker, service_id: &str) -> Result<CreatedService, String> {
    let svc = catalog::find(service_id).ok_or_else(|| format!("Servicio desconocido: {service_id}"))?;

    let result = create_inner(report, docker, &svc).await;
    match &result {
        Ok(c) => emit(report, &svc.id, "done", format!("{} listo", c.name), Some(100.0)),
        Err(e) => emit(report, &svc.id, "error", e.clone(), None),
    }
    result
}

async fn create_inner(report: Reporter<'_>, docker: &Docker, svc: &CatalogService) -> Result<CreatedService, String> {
    ensure_image(report, docker, svc).await?;

    emit(report, &svc.id, "create", "Preparando contenedor…", None);
    let existing = docker
        .list_containers(Some(ListContainersOptionsBuilder::new().all(true).build()))
        .await
        .map_err(|e| format!("No se pudo consultar Docker: {e}"))?;

    let volumes: HashSet<String> = docker
        .list_volumes(None::<bollard::query_parameters::ListVolumesOptions>)
        .await
        .map_err(|e| format!("No se pudo consultar los volúmenes: {e}"))?
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.name)
        .collect();

    let name = pick_name(&svc.id, &existing, &volumes);
    let mut taken = reserved_ports(&existing);
    let mut ports = HashMap::new();
    for p in &svc.ports {
        ports.insert(p.container, pick_port(p.container, &mut taken)?);
    }
    let values = Values { password: catalog::random_password(20), ports };

    let connection = values.render(&svc.connection);
    let web_ui = svc.web_ui.as_deref().map(|w| values.render(w));
    let volume = format!("{name}-data");

    let mut labels = HashMap::from([
        (L_MANAGED.to_string(), "true".to_string()),
        (L_SERVICE.to_string(), svc.id.clone()),
        (L_CONNECTION.to_string(), connection.clone()),
        (L_VOLUME.to_string(), volume.clone()),
        (
            L_PORTS.to_string(),
            values
                .ports
                .iter()
                .map(|(c, h)| format!("{c}={h}"))
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    if let Some(w) = &web_ui {
        labels.insert(L_WEB_UI.to_string(), w.clone());
    }

    // Volumen con nombre para que los datos sobrevivan a recrear el contenedor.
    let mut binds = Vec::new();
    if let Some(path) = &svc.data_path {
        docker
            .create_volume(VolumeCreateRequest {
                name: Some(volume.clone()),
                labels: Some(HashMap::from([
                    (L_MANAGED.to_string(), "true".to_string()),
                    (L_SERVICE.to_string(), svc.id.clone()),
                ])),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("No se pudo crear el volumen: {e}"))?;
        binds.push(format!("{volume}:{path}"));
    }

    // Publica solo en 127.0.0.1: el servicio no queda expuesto a la red local.
    let port_bindings = values
        .ports
        .iter()
        .map(|(c, h)| {
            (
                format!("{c}/tcp"),
                Some(vec![PortBinding {
                    host_ip: Some("127.0.0.1".into()),
                    host_port: Some(h.to_string()),
                }]),
            )
        })
        .collect();

    let second = 1_000_000_000i64; // la API usa nanosegundos
    let body = ContainerCreateBody {
        image: Some(svc.image.clone()),
        env: Some(
            svc.env
                .iter()
                .map(|(k, v)| format!("{k}={}", values.render(v)))
                .collect(),
        ),
        cmd: svc.cmd.as_ref().map(|c| values.render_all(c)),
        exposed_ports: Some(svc.ports.iter().map(|p| format!("{}/tcp", p.container)).collect()),
        labels: Some(labels),
        healthcheck: svc.healthcheck.as_ref().map(|t| HealthConfig {
            test: Some(values.render_all(t)),
            interval: Some(5 * second),
            timeout: Some(5 * second),
            retries: Some(20),
            start_period: Some(5 * second),
            start_interval: Some(second),
        }),
        host_config: Some(HostConfig {
            binds: Some(binds),
            port_bindings: Some(port_bindings),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let created = docker
        .create_container(Some(CreateContainerOptionsBuilder::new().name(&name).build()), body)
        .await
        .map_err(|e| format!("No se pudo crear el contenedor: {e}"))?;

    emit(report, &svc.id, "start", format!("Arrancando {name}…"), None);
    if let Err(e) = docker.start_container(&created.id, None).await {
        // No dejar a medias un contenedor y un volumen recién creados.
        let _ = remove(docker, &created.id, true).await;
        return Err(format!("El contenedor no pudo arrancar y se ha deshecho la creación: {e}"));
    }

    Ok(CreatedService { id: created.id, name, connection, web_ui })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(name: &str, ports_label: Option<&str>) -> ContainerSummary {
        ContainerSummary {
            names: Some(vec![format!("/{name}")]),
            labels: ports_label.map(|p| HashMap::from([(L_PORTS.to_string(), p.to_string())])),
            ..Default::default()
        }
    }

    #[test]
    fn pick_name_adds_suffix_when_taken() {
        let existing = vec![summary("ohc-redis", None), summary("ohc-redis-2", None)];
        let none = HashSet::new();
        assert_eq!(pick_name("redis", &existing, &none), "ohc-redis-3");
        assert_eq!(pick_name("postgres", &existing, &none), "ohc-postgres");
    }

    #[test]
    fn pick_name_skips_names_with_leftover_volume() {
        let volumes = HashSet::from(["ohc-postgres-data".to_string()]);
        assert_eq!(pick_name("postgres", &[], &volumes), "ohc-postgres-2");
    }

    #[test]
    fn reserved_ports_reads_labels_and_published_ports() {
        use bollard::models::PortSummary;
        let mut other = summary("pixels-redis", None);
        other.ports = Some(vec![PortSummary { private_port: 6379, public_port: Some(6379), ..Default::default() }]);
        let existing = vec![summary("a", Some("5432=5433,15672=15672")), other];
        assert_eq!(reserved_ports(&existing), HashSet::from([5433, 15672, 6379]));
    }

    #[test]
    fn pick_port_skips_taken_ports() {
        // Ocupa un puerto real para comprobar que se salta.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let busy = listener.local_addr().unwrap().port();
        let mut taken = HashSet::new();
        let got = pick_port(busy, &mut taken).unwrap();
        assert_ne!(got, busy);
        assert!(taken.contains(&got));
    }
}
