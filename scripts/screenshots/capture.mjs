// Genera las capturas del README con Chrome en modo headless.
//
// Requisitos: Google Chrome instalado y el frontend corriendo (`pnpm dev`).
// Uso:        node scripts/screenshots/capture.mjs
// Resultado:  docs/images/screenshot-<vista>.png (1180×780 a 2x, modo oscuro)

import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const BASE = process.env.APP_URL ?? "http://localhost:1420";
const OUT = new URL("../../docs/images/", import.meta.url).pathname;
const VIEWS = ["services", "catalog", "containers", "logs"];
const PORT = 9333;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const profile = mkdtempSync(join(tmpdir(), "ohc-shots-"));
const chrome = spawn(CHROME, ["--headless", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`, "about:blank"], { stdio: "ignore" });

try {
  // Espera a que Chrome abra el puerto de DevTools.
  let targets;
  for (let i = 0; i < 50 && !targets; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
    } catch {
      await sleep(200);
    }
  }
  if (!targets) throw new Error("Chrome no respondió en el puerto de DevTools");

  const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let seq = 0;
  const pending = new Map();
  ws.addEventListener("message", (e) => {
    const msg = JSON.parse(e.data);
    pending.get(msg.id)?.(msg);
    pending.delete(msg.id);
  });
  const send = (method, params = {}) =>
    new Promise((r) => {
      const id = ++seq;
      pending.set(id, r);
      ws.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true })).result?.result?.value;

  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", { width: 1180, height: 780, deviceScaleFactor: 2, mobile: false });
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });

  for (const view of VIEWS) {
    await send("Page.navigate", { url: `${BASE}/scripts/screenshots/mock.html?view=${view}` });
    let ready = false;
    for (let i = 0; i < 100 && !ready; i++) {
      ready = await evaluate("document.body?.dataset.ready === 'true'");
      if (!ready) await sleep(100);
    }
    if (!ready) throw new Error(`La vista "${view}" no terminó de cargar. ¿Está corriendo pnpm dev?`);
    await sleep(800); // deja terminar transiciones (barra de progreso, scroll)
    const shot = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(OUT, `screenshot-${view}.png`), Buffer.from(shot.result.data, "base64"));
    console.log(`✓ docs/images/screenshot-${view}.png`);
  }
  ws.close();
} finally {
  // Espera a que Chrome se cierre antes de borrar su perfil temporal.
  const exited = chrome.exitCode !== null ? Promise.resolve() : new Promise((r) => chrome.once("exit", r));
  chrome.kill();
  await exited;
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
