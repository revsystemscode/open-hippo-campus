//! Open Hippocampus — PoC de gestor local de servicios en contenedores.
//!
//! Comandos expuestos al frontend (React) mediante `invoke`.

pub mod catalog;
pub mod engine;
pub mod services;

use engine::{Engine, EngineStatus};
use services::{ContainerView, CreatedService};
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
async fn engine_status(engine: State<'_, Engine>) -> Result<EngineStatus, String> {
    Ok(engine.status().await)
}

#[tauri::command]
fn catalog_list() -> Vec<catalog::CatalogService> {
    catalog::all()
}

#[tauri::command]
async fn containers_list(engine: State<'_, Engine>) -> Result<Vec<ContainerView>, String> {
    let conn = engine.connection().await?;
    let result = services::list(&conn.docker).await;
    if result.is_err() {
        // Probablemente el motor se apagó: la próxima llamada buscará otro.
        engine.reset();
    }
    result
}

#[tauri::command]
async fn service_create(
    app: AppHandle,
    engine: State<'_, Engine>,
    service_id: String,
) -> Result<CreatedService, String> {
    let conn = engine.connection().await?;
    let report = move |p: services::Progress| {
        let _ = app.emit(services::PROGRESS_EVENT, p);
    };
    services::create(&report, &conn.docker, &service_id).await
}

#[tauri::command]
async fn container_start(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    services::start(&engine.connection().await?.docker, &id).await
}

#[tauri::command]
async fn container_stop(engine: State<'_, Engine>, id: String) -> Result<(), String> {
    services::stop(&engine.connection().await?.docker, &id).await
}

#[tauri::command]
async fn container_remove(engine: State<'_, Engine>, id: String, delete_data: bool) -> Result<(), String> {
    services::remove(&engine.connection().await?.docker, &id, delete_data).await
}

#[tauri::command]
async fn container_logs(engine: State<'_, Engine>, id: String) -> Result<String, String> {
    services::logs(&engine.connection().await?.docker, &id, 400).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Engine::default())
        .invoke_handler(tauri::generate_handler![
            engine_status,
            catalog_list,
            containers_list,
            service_create,
            container_start,
            container_stop,
            container_remove,
            container_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar la aplicación");
}
