<!-- New service: start a common development service from a recipe. -->
<script>
  import { Button, Modal } from "@elyra/ui";
  import { api } from "../bindings";
  import { startRecipe } from "./recipes.js";

  let { open = $bindable(false) } = $props();
  let recipes = $state([]);
  let chosen = $state(null);
  let name = $state("");
  let busy = $state(false);

  $effect(() => {
    if (open && !recipes.length) api.recipes_list().then((r) => (recipes = r));
    if (open) {
      chosen = null;
      name = "";
    }
  });

  async function start() {
    busy = true;
    const s = await startRecipe(chosen, name.trim() || null);
    busy = false;
    if (s) open = false;
  }
</script>

<Modal bind:open title="New service" description="A database, cache, queue or mail catcher, ready for development: its data in a volume, its port on your Mac, and how to connect copied when it's up." size="lg">
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
    <label class="recipe-name">
      <span class="muted">Name</span>
      <input class="mono" bind:value={name} placeholder={chosen.id} aria-label="Container name" />
    </label>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button variant="primary" disabled={!chosen} loading={busy} onclick={start}>Start {chosen?.title ?? ""}</Button>
  {/snippet}
</Modal>
