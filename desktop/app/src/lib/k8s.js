// Images from eDock into the Kubernetes cluster (`edock k8s load`).
import { toast } from "@elyra/runtime";
import { api } from "../bindings";
import { attempt } from "./state.svelte.js";

// A reference as Docker lists it: `nginx` is `nginx:latest`, Docker Hub's
// prefixes off.
export function shortRef(ref) {
  let r = ref.replace(/^docker\.io\/library\//, "").replace(/^docker\.io\//, "");
  if (!r.includes("@") && !/:[^/]+$/.test(r)) r += ":latest";
  return r;
}

// The images eDock has, by reference.
export async function localImages() {
  const list = await api.images_list().catch(() => []);
  return new Set(list.filter((i) => i.tag !== "<none>").map((i) => shortRef(`${i.repository}:${i.tag}`)));
}

// Load one; true when it's in. What `edock k8s load` said is shown.
export async function loadIntoK8s(image) {
  toast(`Loading ${image} into Kubernetes…`);
  const said = await attempt(() => api.k8s_load(image));
  if (said) toast(said, { variant: "success", duration: 8000 });
  return !!said;
}
