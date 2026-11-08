import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  copyText,
  type CatalogService,
  type ContainerView,
  type EngineStatus,
  type Progress,
} from "./api";

type Tab = "services" | "containers";

interface Toast {
  id: number;
  kind: "ok" | "error";
  text: string;
}

const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));

// ---------------------------------------------------------------------------
// Estado visual de un contenedor
// ---------------------------------------------------------------------------

function stateOf(c: ContainerView): { label: string; tone: "ok" | "warn" | "bad" | "off" } {
  if (c.state === "running") {
    if (c.health === "starting") return { label: "Iniciando", tone: "warn" };
    if (c.health === "unhealthy") return { label: "Con fallos", tone: "bad" };
    if (c.health === "healthy") return { label: "Listo", tone: "ok" };
    return { label: "En ejecución", tone: "ok" };
  }
  if (c.state === "restarting") return { label: "Reiniciando", tone: "warn" };
  if (c.state === "paused") return { label: "Pausado", tone: "warn" };
  if (c.state === "created") return { label: "Creado", tone: "off" };
  return { label: "Detenido", tone: "off" };
}

function portsText(c: ContainerView): string {
  const published = c.ports.filter((p) => p.publicPort);
  if (published.length === 0) return "—";
  return published.map((p) => `${p.publicPort}→${p.privatePort}`).join(", ");
}

function formatBytes(b: number | null): string {
  if (!b) return "—";
  return `${(b / 1024 ** 3).toFixed(1)} GB`;
}

// ---------------------------------------------------------------------------
// Componentes pequeños
// ---------------------------------------------------------------------------

function StateBadge({ c }: { c: ContainerView }) {
  const s = stateOf(c);
  return (
    <span className={`badge badge-${s.tone}`}>
      <span className="dot" />
      {s.label}
    </span>
  );
}

function CopyField({ value, onCopied }: { value: string; onCopied: () => void }) {
  return (
    <div className="copy-field">
      <code title={value}>{value}</code>
      <button
        className="btn btn-ghost btn-sm"
        onClick={async () => {
          await copyText(value);
          onCopied();
        }}
      >
        Copiar
      </button>
    </div>
  );
}

function EngineBadge({ status, onRetry }: { status: EngineStatus | null; onRetry: () => void }) {
  if (!status) return <span className="engine engine-wait">Buscando motor…</span>;
  if (!status.connected)
    return (
      <span className="engine engine-off">
        <span className="dot" /> Sin motor de contenedores
        <button className="btn btn-ghost btn-sm" onClick={onRetry}>
          Reintentar
        </button>
      </span>
    );
  return (
    <span className="engine engine-on" title={`${status.endpoint ?? ""}\nKernel: ${status.kernel ?? "—"}`}>
      <span className="dot" />
      {status.provider} · Docker {status.serverVersion} · {status.running ?? 0} en ejecución
    </span>
  );
}

// ---------------------------------------------------------------------------
// Catálogo
// ---------------------------------------------------------------------------

function CatalogCard({
  svc,
  progress,
  disabled,
  onCreate,
}: {
  svc: CatalogService;
  progress: Progress | undefined;
  disabled: boolean;
  onCreate: () => void;
}) {
  const busy = progress && progress.phase !== "done" && progress.phase !== "error";
  return (
    <article className="card">
      <header className="card-head">
        <div className={`logo logo-${svc.id}`}>{svc.name.slice(0, 2)}</div>
        <div>
          <h3>{svc.name}</h3>
          <p className="muted small">{svc.category}</p>
        </div>
      </header>
      <p className="card-desc">{svc.description}</p>
      <dl className="meta">
        <dt>Imagen</dt>
        <dd>
          <code>{svc.image}</code>
        </dd>
        <dt>Puertos</dt>
        <dd>{svc.ports.map((p) => `${p.container} (${p.label})`).join(", ")}</dd>
      </dl>
      {busy ? (
        <div className="progress">
          <div className="progress-bar">
            <div
              className={`progress-fill ${progress.percent == null ? "indeterminate" : ""}`}
              style={progress.percent != null ? { width: `${progress.percent}%` } : undefined}
            />
          </div>
          <p className="small muted">{progress.message}</p>
        </div>
      ) : (
        <button className="btn btn-primary" disabled={disabled} onClick={onCreate}>
          Crear {svc.name}
        </button>
      )}
    </article>
  );
}

// ---------------------------------------------------------------------------
// Fila de un servicio gestionado
// ---------------------------------------------------------------------------

function ServiceRow({
  c,
  busy,
  onAction,
  onLogs,
  onRemove,
  toast,
}: {
  c: ContainerView;
  busy: boolean;
  onAction: (id: string, action: "start" | "stop") => void;
  onLogs: (c: ContainerView) => void;
  onRemove: (c: ContainerView) => void;
  toast: (t: Omit<Toast, "id">) => void;
}) {
  const running = c.state === "running" || c.state === "restarting";
  return (
    <div className="service-row">
      <div className="service-main">
        <div className={`logo logo-sm logo-${c.serviceId}`}>{(c.serviceName ?? c.name).slice(0, 2)}</div>
        <div className="service-title">
          <strong>{c.name}</strong>
          <span className="muted small">
            {c.serviceName} · {c.image} · puertos {portsText(c)}
          </span>
        </div>
        <StateBadge c={c} />
        <div className="actions">
          {running ? (
            <button className="btn btn-sm" disabled={busy} onClick={() => onAction(c.id, "stop")}>
              Detener
            </button>
          ) : (
            <button className="btn btn-sm" disabled={busy} onClick={() => onAction(c.id, "start")}>
              Arrancar
            </button>
          )}
          <button className="btn btn-sm" onClick={() => onLogs(c)}>
            Logs
          </button>
          <button className="btn btn-sm btn-danger" disabled={busy} onClick={() => onRemove(c)}>
            Borrar
          </button>
        </div>
      </div>
      {c.connection && (
        <div className="service-conn">
          <span className="muted small">Conexión</span>
          <CopyField value={c.connection} onCopied={() => toast({ kind: "ok", text: "Cadena de conexión copiada" })} />
          {c.webUi && (
            <>
              <span className="muted small">Panel web</span>
              <CopyField value={c.webUi} onCopied={() => toast({ kind: "ok", text: "URL copiada" })} />
            </>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Diálogos
// ---------------------------------------------------------------------------

function LogsDialog({ c, onClose }: { c: ContainerView; onClose: () => void }) {
  const [text, setText] = useState<string>("Cargando…");
  const preRef = useRef<HTMLPreElement>(null);

  const load = useCallback(async () => {
    try {
      const t = await api.logs(c.id);
      setText(t || "(sin salida)");
    } catch (e) {
      setText(errText(e));
    }
  }, [c.id]);

  useEffect(() => {
    load();
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  }, [load]);

  useEffect(() => {
    const el = preRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text]);

  return (
    <div className="overlay" onClick={onClose}>
      <div className="dialog dialog-wide" onClick={(e) => e.stopPropagation()}>
        <header className="dialog-head">
          <h3>Logs · {c.name}</h3>
          <button className="btn btn-ghost btn-sm" onClick={onClose}>
            Cerrar
          </button>
        </header>
        <pre ref={preRef} className="logs">
          {text}
        </pre>
        <p className="small muted">Últimas 400 líneas · se actualiza cada 3 s</p>
      </div>
    </div>
  );
}

function RemoveDialog({
  c,
  onCancel,
  onConfirm,
}: {
  c: ContainerView;
  onCancel: () => void;
  onConfirm: (deleteData: boolean) => void;
}) {
  const [deleteData, setDeleteData] = useState(false);
  return (
    <div className="overlay" onClick={onCancel}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>¿Borrar {c.name}?</h3>
        <p className="muted">El contenedor se detendrá y se eliminará.</p>
        {c.managed && (
          <label className="check">
            <input type="checkbox" checked={deleteData} onChange={(e) => setDeleteData(e.target.checked)} />
            Borrar también los datos (volumen <code>{c.name}-data</code>). No se puede deshacer.
          </label>
        )}
        <div className="dialog-actions">
          <button className="btn" onClick={onCancel}>
            Cancelar
          </button>
          <button className="btn btn-danger" onClick={() => onConfirm(deleteData)}>
            Borrar
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

export default function App() {
  const [tab, setTab] = useState<Tab>("services");
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [catalog, setCatalog] = useState<CatalogService[]>([]);
  const [containers, setContainers] = useState<ContainerView[]>([]);
  const [progress, setProgress] = useState<Record<string, Progress>>({});
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const [logsFor, setLogsFor] = useState<ContainerView | null>(null);
  const [removeFor, setRemoveFor] = useState<ContainerView | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);

  const toast = useCallback((t: Omit<Toast, "id">) => {
    const id = Date.now() + Math.random();
    setToasts((all) => [...all, { ...t, id }]);
    setTimeout(() => setToasts((all) => all.filter((x) => x.id !== id)), t.kind === "error" ? 7000 : 2500);
  }, []);

  const refreshEngine = useCallback(async () => {
    try {
      setEngine(await api.engineStatus());
    } catch (e) {
      toast({ kind: "error", text: errText(e) });
    }
  }, [toast]);

  const refreshContainers = useCallback(async () => {
    try {
      setContainers(await api.containers());
    } catch {
      // Sin motor: el indicador superior ya lo muestra.
      setContainers([]);
    }
  }, []);

  // Carga inicial y sondeo periódico.
  useEffect(() => {
    api.catalog().then(setCatalog).catch((e) => toast({ kind: "error", text: errText(e) }));
    refreshEngine();
    const e = setInterval(refreshEngine, 10000);
    return () => clearInterval(e);
  }, [refreshEngine, toast]);

  useEffect(() => {
    if (!engine?.connected) return;
    refreshContainers();
    const t = setInterval(refreshContainers, 3000);
    return () => clearInterval(t);
  }, [engine?.connected, refreshContainers]);

  // Progreso de creación emitido por el backend.
  useEffect(() => {
    const unlisten = api.onProgress((p) => setProgress((all) => ({ ...all, [p.serviceId]: p })));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const createService = async (svc: CatalogService) => {
    setProgress((all) => ({
      ...all,
      [svc.id]: { serviceId: svc.id, phase: "pull", message: "Iniciando…", percent: null },
    }));
    try {
      const created = await api.createService(svc.id);
      toast({ kind: "ok", text: `${created.name} creado. Esperando a que esté listo…` });
      await refreshContainers();
      refreshEngine();
    } catch (e) {
      toast({ kind: "error", text: errText(e) });
    } finally {
      setProgress((all) => {
        const next = { ...all };
        delete next[svc.id];
        return next;
      });
    }
  };

  const withBusy = async (id: string, fn: () => Promise<void>) => {
    setBusy((s) => new Set(s).add(id));
    try {
      await fn();
      await refreshContainers();
      refreshEngine();
    } catch (e) {
      toast({ kind: "error", text: errText(e) });
    } finally {
      setBusy((s) => {
        const n = new Set(s);
        n.delete(id);
        return n;
      });
    }
  };

  const onAction = (id: string, action: "start" | "stop") =>
    withBusy(id, () => (action === "start" ? api.start(id) : api.stop(id)));

  const onRemoveConfirm = (deleteData: boolean) => {
    const c = removeFor;
    setRemoveFor(null);
    if (!c) return;
    withBusy(c.id, async () => {
      await api.remove(c.id, deleteData);
      toast({ kind: "ok", text: `${c.name} borrado${deleteData ? " junto con sus datos" : ""}` });
    });
  };

  const managed = useMemo(() => containers.filter((c) => c.managed), [containers]);
  const connected = engine?.connected ?? false;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img src="/app-icon.svg" alt="" width={28} height={28} />
          <span>Open Hippo Campus</span>
        </div>
        <EngineBadge status={engine} onRetry={refreshEngine} />
      </header>

      <nav className="tabs">
        <button className={tab === "services" ? "tab active" : "tab"} onClick={() => setTab("services")}>
          Servicios <span className="count">{managed.length}</span>
        </button>
        <button className={tab === "containers" ? "tab active" : "tab"} onClick={() => setTab("containers")}>
          Todos los contenedores <span className="count">{containers.length}</span>
        </button>
      </nav>

      <main className="content">
        {!connected && engine && (
          <section className="notice">
            <h3>No hay un motor de contenedores en ejecución</h3>
            <p>
              Esta versión se conecta a un motor existente. Abre <strong>OrbStack</strong>, <strong>Docker Desktop</strong>,{" "}
              <strong>Colima</strong> o <strong>Rancher Desktop</strong> y pulsa Reintentar.
            </p>
            {engine.tried.length > 0 && <p className="small muted">Probado: {engine.tried.join(" · ")}</p>}
          </section>
        )}

        {tab === "services" && (
          <>
            <section>
              <h2>Mis servicios</h2>
              {managed.length === 0 ? (
                <p className="empty">Aún no has creado ningún servicio. Elige uno del catálogo.</p>
              ) : (
                <div className="service-list">
                  {managed.map((c) => (
                    <ServiceRow
                      key={c.id}
                      c={c}
                      busy={busy.has(c.id)}
                      onAction={onAction}
                      onLogs={setLogsFor}
                      onRemove={setRemoveFor}
                      toast={toast}
                    />
                  ))}
                </div>
              )}
            </section>

            <section>
              <h2>Catálogo</h2>
              <div className="grid">
                {catalog.map((svc) => (
                  <CatalogCard
                    key={svc.id}
                    svc={svc}
                    progress={progress[svc.id]}
                    disabled={!connected}
                    onCreate={() => createService(svc)}
                  />
                ))}
              </div>
            </section>
          </>
        )}

        {tab === "containers" && (
          <section>
            <h2>Todos los contenedores</h2>
            {engine?.connected && (
              <p className="small muted">
                {engine.os} · {engine.arch} · {engine.cpus} CPU · {formatBytes(engine.memoryBytes)} · {engine.images} imágenes
              </p>
            )}
            {containers.length === 0 ? (
              <p className="empty">No hay contenedores.</p>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>Nombre</th>
                    <th>Imagen</th>
                    <th>Estado</th>
                    <th>Puertos</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {containers.map((c) => {
                    const running = c.state === "running" || c.state === "restarting";
                    return (
                      <tr key={c.id}>
                        <td>
                          <strong>{c.name}</strong>
                          {c.composeProject && <span className="tag">compose: {c.composeProject}</span>}
                          {c.managed && <span className="tag tag-accent">app</span>}
                          <div className="muted small">{c.status}</div>
                        </td>
                        <td>
                          <code>{c.image}</code>
                        </td>
                        <td>
                          <StateBadge c={c} />
                        </td>
                        <td>{portsText(c)}</td>
                        <td>
                          <div className="actions">
                          <button
                            className="btn btn-sm"
                            disabled={busy.has(c.id)}
                            onClick={() => onAction(c.id, running ? "stop" : "start")}
                          >
                            {running ? "Detener" : "Arrancar"}
                          </button>
                          <button className="btn btn-sm" onClick={() => setLogsFor(c)}>
                            Logs
                          </button>
                          <button
                            className="btn btn-sm btn-danger"
                            disabled={busy.has(c.id)}
                            onClick={() => setRemoveFor(c)}
                          >
                            Borrar
                          </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </section>
        )}
      </main>

      {logsFor && <LogsDialog c={logsFor} onClose={() => setLogsFor(null)} />}
      {removeFor && <RemoveDialog c={removeFor} onCancel={() => setRemoveFor(null)} onConfirm={onRemoveConfirm} />}

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast toast-${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
