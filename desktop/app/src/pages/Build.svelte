<!-- One build: its steps, what was cached, each step's log, and why it failed. -->
<script>
  import { confirm, navigate, shell } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { attempt, errorText } from "../lib/state.svelte.js";
  import { bytes } from "../lib/format.js";

  let { params } = $props();
  let b = $state(null);
  let error = $state(null);
  let open = $state({});

  $effect(() => {
    api
      .build_detail(decodeURIComponent(params.builder), params.id)
      .then((d) => {
        b = d;
        // A failed step opens by itself.
        d.steps.forEach((s, i) => s.error && (open[i] = true));
      })
      .catch((e) => (error = errorText(e)));
  });

  // The image it made, and how it compares with the build before.
  let image = $state(null);
  $effect(() => {
    if (b?.status !== "Completed") return;
    api.build_image(b.builder, b.id).then((i) => (image = i)).catch(() => {});
  });
  const change = $derived(image?.previous_size != null ? image.size - image.previous_size : null);
  const openImage = (tab = "") => navigate(`/images/${encodeURIComponent(image.reference)}${tab}`);

  const secs = (s) => {
    if (!s.started || !s.completed || s.completed.startsWith("0001-")) return null;
    const t = (Date.parse(s.completed) - Date.parse(s.started)) / 1000;
    return t >= 0 ? t : null;
  };
  const TONE = { Completed: "success", Running: "accent", Error: "danger", Canceled: "warning" };

  async function remove() {
    if (!(await confirm("Take this build off the history?", { confirmLabel: "Remove" }))) return;
    if (await attempt(() => api.build_remove(b.builder, b.id).then(() => true))) navigate("/builds");
  }
</script>

<div class="page">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/builds")}><Icon name="back" /></button>
    <h1 class="mono">{b?.name ?? "Build"}</h1>
    {#if b}<Badge variant={TONE[b.status] ?? "neutral"}>{b.status}</Badge>{/if}
    <span class="spacer"></span>
    {#if b}<Button size="sm" variant="ghost" onclick={remove}><Icon name="trash" size={14} /> Remove</Button>{/if}
  </header>

  {#if error}
    <Callout variant="warning" title="Couldn't read the build">{error}</Callout>
  {:else if !b}
    <Skeleton lines={6} />
  {:else}
    <div class="facts">
      <div><span class="muted">Steps</span>{b.total} ({b.cached} cached)</div>
      {#if b.dockerfile}<div><span class="muted">Dockerfile</span><span class="mono">{b.dockerfile}</span></div>{/if}
      {#if b.context}<div><span class="muted">Context</span><span class="mono">{b.context}</span></div>{/if}
      {#if image}
        <div>
          <span class="muted">Image</span>
          <span>
            {bytes(image.size)}
            {#if change != null && Math.abs(change) >= 1024}
              <Badge variant={change < 0 ? "success" : "warning"} title="Against the last build of {b.name}">{change < 0 ? "−" : "+"}{bytes(Math.abs(change))}</Badge>
            {:else if change != null}
              <span class="muted">(as before)</span>
            {/if}
          </span>
        </div>
      {/if}
    </div>

    {#if image}
      <div class="chips">
        {#if image.reference}
          <Button size="sm" onclick={() => openImage()}><Icon name="layers" size={14} /> Inspect image</Button>
          <Button size="sm" onclick={() => openImage("?tab=security")}><Icon name="shield" size={14} /> Scan</Button>
        {:else}
          <span class="muted">Its image isn't in eDock under a name any more: built again since, or removed.</span>
        {/if}
      </div>
    {/if}

    {#if b.error}
      <Callout variant="danger" title={b.error_step ? `Failed at ${b.error_step}` : "The build failed"}>
        <pre class="build-error">{b.error}</pre>
        {#if b.error_source}<pre class="build-error muted">{b.error_source}</pre>{/if}
      </Callout>
    {/if}

    {#each b.warnings as w, i (i)}
      <Callout variant="warning" title={w.text}>
        {w.detail}{#if w.line} (line {w.line}){/if}
        {#if w.url}<button class="link" onclick={() => shell.openExternal(w.url)}>More</button>{/if}
      </Callout>
    {/each}

    <ul class="steps">
      {#each b.steps as s, i (i)}
        <li class:failed={s.error}>
          <button class="step-head" onclick={() => (open[i] = !open[i])} disabled={!s.log && !s.error}>
            <span class="mono step-name">{s.name}</span>
            {#if s.cached}<Badge>cached</Badge>{/if}
            {#if s.error}<Badge variant="danger">failed</Badge>{/if}
            <span class="muted step-time">{secs(s) != null ? `${secs(s).toFixed(1)} s` : ""}</span>
          </button>
          {#if open[i] && (s.log || s.error)}
            <pre class="step-log">{s.log}{s.error ? `\n${s.error}` : ""}</pre>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>
