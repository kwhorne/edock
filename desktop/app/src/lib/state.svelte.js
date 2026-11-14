// What every page shares: the engine's status, and a tick per kind of
// thing that changed in the engine (so a list reloads when its kind does).
import { channel, toast } from "@elyra/runtime";
import { api } from "../bindings";

export const engine = $state({ status: null, contexts: [], domains: null });

// One counter per kind; a page reads the one it lists in an $effect.
export const changes = $state({ container: 0, image: 0, volume: 0, network: 0, engine: 0, k8s: 0 });

export async function refreshEngine() {
  try {
    const [status, contexts, domains] = await Promise.all([api.engine_status(), api.contexts_list(), api.domains_info()]);
    engine.status = status;
    engine.contexts = contexts;
    engine.domains = domains;
  } catch (e) {
    console.error(e);
  }
}

let started = false;
export function startWatching() {
  if (started) return;
  started = true;
  refreshEngine();
  channel("docker:changed").subscribe((c) => {
    if (!c) return;
    if (c.kind === "engine" || c.kind === "context") {
      changes.engine++;
      // A new engine: every list.
      changes.container++;
      changes.image++;
      changes.volume++;
      changes.network++;
      changes.k8s++;
      refreshEngine();
      return;
    }
    if (c.kind in changes) changes[c.kind]++;
    // A container that comes or goes changes what images and volumes are used by.
    if (c.kind === "container" && (c.action === "create" || c.action === "destroy")) {
      changes.image++;
      changes.volume++;
      changes.network++;
    }
    if (c.kind === "container") refreshEngineSoon();
  });
}

let pending = null;
function refreshEngineSoon() {
  clearTimeout(pending);
  pending = setTimeout(refreshEngine, 400);
}

/** Run `fn`, and toast its failure. Returns what it returned, or undefined. */
export async function attempt(fn, done) {
  try {
    const r = await fn();
    if (done) toast(done, { variant: "success" });
    return r;
  } catch (e) {
    toast(e?.message ?? String(e), { variant: "danger", duration: 6000 });
  }
}

export async function useContext(name) {
  await attempt(() => api.context_use(name));
  await refreshEngine();
}

/** A container's address at eDock's router, or null: https when browsers
 *  trust eDock's certificates, else http; the port only when it isn't the
 *  scheme's own. */
export function domainUrl(row) {
  const d = engine.domains;
  if (!row?.domain || !d?.available || row.state !== "running") return null;
  const https = d.trusted && d.https_on;
  if (!https && !d.http_on) return null;
  const port = https ? (d.https_port === 443 ? "" : `:${d.https_port}`) : d.http_port === 80 ? "" : `:${d.http_port}`;
  return `${https ? "https" : "http"}://${row.domain}.${d.suffix}${port}`;
}
