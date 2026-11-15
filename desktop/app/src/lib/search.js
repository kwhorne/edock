// ⌘K finds containers, images, Linux machines and Kubernetes pods by name.
// Each list is read at most every few seconds while typing.
import { navigate, registerSearch } from "@elyra/runtime";
import { api } from "../bindings";
import { engine } from "./state.svelte.js";

function cached(read, ms = 3000) {
  let at = 0;
  let value = null;
  return async () => {
    if (!value || Date.now() - at > ms) {
      value = await read();
      at = Date.now();
    }
    return value;
  };
}

const has = (q, ...fields) => fields.some((f) => f && f.toLowerCase().includes(q));
const LIMIT = 8;

const containers = cached(() => api.containers_list(false));
const images = cached(() => api.images_list());
const machines = cached(() => api.machines_list().catch(() => []));
const pods = cached(async () => {
  const s = await api.k8s_status();
  return s.on_edock && s.ready ? (await api.k8s_items()).pods : [];
}, 5000);

const recipes = cached(() => api.recipes_list(), 60000);

export function startSearch(startRecipe) {
  registerSearch({
    id: "containers",
    title: "Containers",
    minChars: 1,
    async search(query) {
      const q = query.trim().toLowerCase();
      return (await containers())
        .filter((c) => has(q, c.name, c.image, c.project, c.id.slice(0, 12)))
        .slice(0, LIMIT)
        .map((c) => ({ title: c.name, subtitle: `${c.state} · ${c.image}${c.project ? ` · ${c.project}` : ""}`, action: () => navigate(`/containers/${c.id}`) }));
    },
  });
  registerSearch({
    id: "images",
    title: "Images",
    async search(query) {
      const q = query.trim().toLowerCase();
      return (await images())
        .filter((i) => i.tag !== "<none>" && has(q, `${i.repository}:${i.tag}`))
        .slice(0, LIMIT)
        .map((i) => {
          const ref = `${i.repository}:${i.tag}`;
          return { title: ref, subtitle: "Image", action: () => navigate(`/images/${encodeURIComponent(ref)}`) };
        });
    },
  });
  registerSearch({
    id: "machines",
    title: "Linux machines",
    async search(query) {
      if (engine.status?.context !== "edock") return [];
      const q = query.trim().toLowerCase();
      return (await machines())
        .filter((m) => has(q, m.name, m.distro))
        .slice(0, LIMIT)
        .map((m) => ({ title: m.name, subtitle: `${m.distro} · ${m.state}`, action: () => navigate(`/machines/${m.name}`) }));
    },
  });
  registerSearch({
    id: "pods",
    title: "Kubernetes pods",
    async search(query) {
      if (engine.status?.context !== "edock") return [];
      const q = query.trim().toLowerCase();
      return (await pods().catch(() => []))
        .filter((p) => has(q, p.name, p.namespace, p.owner))
        .slice(0, LIMIT)
        .map((p) => ({ title: p.name, subtitle: `${p.namespace} · ${p.status}`, action: () => navigate(`/kubernetes/${p.namespace}/${p.name}`) }));
    },
  });
  registerSearch({
    id: "recipes",
    title: "New service",
    minChars: 1,
    async search(query) {
      const q = query.trim().toLowerCase();
      return (await recipes())
        .filter((r) => has(q, r.title, r.id, r.image, "new service database recipe"))
        .map((r) => ({ title: `New service: ${r.title}`, subtitle: r.image, action: () => startRecipe(r) }));
    },
  });
}
