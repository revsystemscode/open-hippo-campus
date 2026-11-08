// Tipos y llamadas al backend Rust (comandos de Tauri).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface EngineStatus {
  connected: boolean;
  endpoint: string | null;
  provider: string | null;
  serverVersion: string | null;
  os: string | null;
  kernel: string | null;
  arch: string | null;
  cpus: number | null;
  memoryBytes: number | null;
  containers: number | null;
  running: number | null;
  images: number | null;
  error: string | null;
  tried: string[];
}

export interface ServicePort {
  container: number;
  label: string;
}

export interface CatalogService {
  id: string;
  name: string;
  category: string;
  description: string;
  image: string;
  ports: ServicePort[];
  webUi: string | null;
}

export interface PortView {
  privatePort: number;
  publicPort: number | null;
}

export interface ContainerView {
  id: string;
  name: string;
  image: string;
  state: string;
  status: string;
  health: string | null;
  ports: PortView[];
  created: number;
  managed: boolean;
  serviceId: string | null;
  serviceName: string | null;
  connection: string | null;
  webUi: string | null;
  composeProject: string | null;
}

export interface Progress {
  serviceId: string;
  phase: "pull" | "create" | "start" | "done" | "error";
  message: string;
  percent: number | null;
}

export interface CreatedService {
  id: string;
  name: string;
  connection: string;
  webUi: string | null;
}

export const api = {
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  catalog: () => invoke<CatalogService[]>("catalog_list"),
  containers: () => invoke<ContainerView[]>("containers_list"),
  createService: (serviceId: string) => invoke<CreatedService>("service_create", { serviceId }),
  start: (id: string) => invoke<void>("container_start", { id }),
  stop: (id: string) => invoke<void>("container_stop", { id }),
  remove: (id: string, deleteData: boolean) => invoke<void>("container_remove", { id, deleteData }),
  logs: (id: string) => invoke<string>("container_logs", { id }),
  onProgress: (cb: (p: Progress) => void): Promise<UnlistenFn> =>
    listen<Progress>("service-progress", (e) => cb(e.payload)),
};

/** Copia texto al portapapeles, con alternativa para webviews sin permiso. */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.appendChild(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
}
