<!-- docker build's cache, bringing things over from another engine, and a
     diagnostics report. -->
<script>
  import { channel, confirm, dialog, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Select } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import { attempt, changes } from "./state.svelte.js";
  import { bytes } from "./format.js";

  // ── Builds ──
  let builder = $state(null);
  let bform = $state({ cache: "", idle: "" });
  let busy = $state(null);
  async function loadBuilder() {
    try {
      builder = await api.builder_info();
      bform = { cache: builder.cache_gb ? String(builder.cache_gb) : "", idle: String(Math.round(builder.idle_secs / 60)) };
    } catch {
      builder = null;
    }
  }
  $effect(() => {
    changes.engine;
    loadBuilder();
  });
  async function saveBuilder(e) {
    e.preventDefault();
    const cache = bform.cache.trim() === "" ? null : Number(bform.cache);
    const idle = Number(bform.idle);
    if ((cache !== null && !(cache > 0)) || !(idle > 0)) return toast("Use whole numbers above 0", { variant: "danger" });
    busy = "builder";
    const b = await attempt(() => api.builder_set(cache, idle * 60), "Saved: the builder uses it from its next start");
    busy = null;
    if (b) builder = b;
  }
  async function prune() {
    if (!(await confirm("Empty the build cache? The next builds start from scratch.", { confirmLabel: "Empty" }))) return;
    busy = "prune";
    const freed = await attempt(() => api.builder_prune());
    busy = null;
    if (freed != null) toast(`Freed ${bytes(freed)}`, { variant: "success" });
  }

  // ── Import ──
  let sources = $state([]);
  let source = $state(null);
  let list = $state(null);
  let lines = $state([]);
  let importing = $state(false);
  api.import_sources().then((s) => {
    sources = s;
    source = s.find((x) => x.available)?.id ?? null;
  });
  $effect(() => {
    if (source) look();
  });
  async function look() {
    list = null;
    list = await attempt(() => api.import_list(source));
  }
  const fresh = (items) => items?.filter((i) => !i.have).length ?? 0;
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const off = channel("import:progress").subscribe((p) => {
    if (!p) return;
    if (p.line) lines = [...lines.slice(-40), p.line];
    if (p.done) {
      importing = false;
      if (p.error) toast(p.error, { variant: "danger", duration: 8000 });
      else toast("Imported", { variant: "success" });
      look();
    }
  });
  onDestroy(off);
  async function runImport() {
    const label = sources.find((s) => s.id === source)?.label;
    if (!(await confirm(`Bring ${plural(fresh(list.images), "image")}, ${plural(fresh(list.volumes), "volume")} and ${plural(fresh(list.containers), "container")} over from ${label}? ${label} is only read; containers arrive stopped.`, { confirmLabel: "Import" }))) return;
    lines = [];
    importing = true;
    const ok = await attempt(() => api.import_run(source).then(() => true));
    if (!ok) importing = false;
  }

  // ── Diagnostics ──
  async function report() {
    const file = await dialog.save({ title: "Save a diagnostics report", defaultName: "edock-diagnostics.txt" });
    if (file) await attempt(() => api.diag_report(file), "Report saved");
  }
</script>

<Card title="Builds">
  {#if builder}
    <form class="port-form" onsubmit={saveBuilder}>
      <label>Cache limit (GB) <input bind:value={bform.cache} inputmode="numeric" placeholder={`${builder.default_cache_gb} (default)`} /></label>
      <label>Stop when idle (min) <input bind:value={bform.idle} inputmode="numeric" /></label>
      <Button type="submit" size="sm" loading={busy === "builder"}>Save</Button>
    </form>
    <p class="muted text">
      <code>docker build</code> runs on BuildKit in an eDock container, which stops when it has been idle that long. It's
      <Badge variant={builder.state === "running" ? "success" : "neutral"}>{builder.state === "running" ? "running" : "not running"}</Badge> now.
    </p>
  {:else}
    <p class="muted">The builder's settings are eDock's own engine's.</p>
  {/if}
  {#snippet actions()}
    {#if builder}<Button size="sm" loading={busy === "prune"} onclick={prune}>Empty cache</Button>{/if}
  {/snippet}
</Card>

<Card title="Import">
  <div class="engine-card">
    {#if !sources.some((s) => s.available)}
      <p class="muted">Bring images, volumes and containers over from OrbStack or Docker Desktop while it runs.</p>
    {:else}
      <div class="port-form">
        <label>From
          <Select value={source} options={sources.filter((s) => s.available).map((s) => ({ value: s.id, label: s.label }))} onchange={(v) => (source = v)} />
        </label>
      </div>
      {#if list}
        <p>
          <Badge>{plural(fresh(list.images), "image")}</Badge> <Badge>{plural(fresh(list.volumes), "volume")}</Badge> <Badge>{plural(fresh(list.containers), "container")}</Badge>
          <span class="muted">not in eDock yet</span>
        </p>
      {/if}
      <p class="muted text">The other engine is only read. Volumes keep their files' owners; containers arrive stopped.</p>
      {#if importing || lines.length}<pre class="progress">{lines.join("\n") || "…"}</pre>{/if}
    {/if}
  </div>
  {#snippet actions()}
    {#if list}
      <Button size="sm" variant="primary" loading={importing} disabled={!fresh(list.images) && !fresh(list.volumes) && !fresh(list.containers)} onclick={runImport}>Import</Button>
    {/if}
  {/snippet}
</Card>

<Card title="Diagnostics">
  <p class="muted text">A report of eDock's state and recent logs, with tokens taken out, for when something's wrong. It's saved where you choose; nothing is sent.</p>
  {#snippet actions()}<Button size="sm" onclick={report}>Save report…</Button>{/snippet}
</Card>
