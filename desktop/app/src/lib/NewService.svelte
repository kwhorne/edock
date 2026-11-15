<!-- New service: start a common development service from a recipe. -->
<script>
  import { dialog } from "@elyra/runtime";
  import { Button, Modal } from "@elyra/ui";
  import { api } from "../bindings";
  import { startRecipe } from "./recipes.js";

  let { open = $bindable(false) } = $props();
  let recipes = $state([]);
  let chosen = $state(null);
  let name = $state("");
  let folder = $state(null);
  async function pickFolder() {
    const [dir] = await dialog.open({ title: `The project ${chosen.title} serves`, directory: true });
    if (dir) folder = dir.path;
  }
  let busy = $state(false);

  $effect(() => {
    if (open && !recipes.length) api.recipes_list().then((r) => (recipes = r));
    if (open) {
      chosen = null;
      name = "";
      folder = null;
    }
  });

  async function start() {
    busy = true;
    const s = await startRecipe(chosen, name.trim() || null, folder);
    busy = false;
    if (s) open = false;
  }
</script>

<Modal bind:open title="New service" description="A database, cache, queue, mail catcher or app server, ready for development: its data in a volume, its port on your Mac, and how to connect copied when it's up." size="lg">
  <div class="recipes">
    {#each recipes as r (r.id)}
      <button class="recipe" class:chosen={chosen?.id === r.id} onclick={() => (chosen = r)}>
        <strong>{r.title}</strong>
        <span class="muted">{r.about}</span>
        <span class="muted mono small">{r.image}</span>
      </button>
    {/each}
  </div>
  {#if chosen}
    {#if chosen.needs_folder}
      <div class="recipe-name">
        <span class="muted">Project</span>
        <span class="mono folder-path">{folder ?? "No folder chosen"}</span>
        <Button size="sm" onclick={pickFolder}>Choose folder…</Button>
      </div>
      <p class="muted small">Laravel (a folder with <code>artisan</code>) runs in worker mode; any other PHP is served from its <code>public/</code> folder. It runs as you, so it can write <code>storage/</code>. Composer's <code>vendor/</code> must be installed already. A database on your Mac (one of these services, say) is
        <code>DB_HOST=host.docker.internal</code> from inside it, not <code>127.0.0.1</code>.</p>
    {/if}
    <label class="recipe-name">
      <span class="muted">Name</span>
      <input class="mono" bind:value={name} placeholder={chosen.needs_folder ? (folder?.split("/").pop()?.toLowerCase().replace(/[^a-z0-9]+/g, "-") || "the folder's name") : chosen.id} aria-label="Container name" />
    </label>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button variant="primary" disabled={!chosen || (chosen.needs_folder && !folder)} loading={busy} onclick={start}>Start {chosen?.title ?? ""}</Button>
  {/snippet}
</Modal>
