// The app's pages: a pattern ("/containers/:id") -> a loader. The most
// specific pattern wins.
export const routes = {
  "/": () => import("./pages/Containers.svelte"),
  "/containers/:id": () => import("./pages/Container.svelte"),
  "/images": () => import("./pages/Images.svelte"),
  "/volumes": () => import("./pages/Volumes.svelte"),
  "/networks": () => import("./pages/Networks.svelte"),
  "/engine": () => import("./pages/Engine.svelte"),
  "/kubernetes": () => import("./pages/Kubernetes.svelte"),
  "/kubernetes/:namespace/:pod": () => import("./pages/Pod.svelte"),
  // The menu bar item's popover.
  "/quick": () => import("./pages/Quick.svelte"),
};
