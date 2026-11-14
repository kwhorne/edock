import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// `base: "./"` -> relative asset URLs (served from elyra://localhost/).
export default defineConfig({
  plugins: [svelte()],
  base: "./",
  // One Svelte and one @elyra/runtime, the app's own, even when @elyra/ui is
  // linked from a checkout (`rata new --elyra`): two runtimes would keep two
  // sets of stores.
  resolve: { dedupe: ["svelte", "@elyra/runtime"] },
  build: { outDir: "dist", emptyOutDir: true, target: "esnext" },
  server: { port: 5173, strictPort: true },
});
