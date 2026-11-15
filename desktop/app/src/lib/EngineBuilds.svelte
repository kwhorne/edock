<!-- docker build's cache, bringing things over from another engine, and a
     diagnostics report. -->
<script>
  import { channel, clipboard, confirm, dialog, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Modal, Select, Switch } from "@elyra/ui";
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
    if ((cache !== null && !(cache > 0)) || !(idle > 0)) return toast("Use whole numbers above 0", { variant: "error" });
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
  // Which builder plain `docker build` uses (`edock build-with`).
  let buildWith = $state(null);
  api.build_with().then((b) => (buildWith = b)).catch(() => {});
  async function setBuildWith(v) {
    const b = await attempt(() => api.build_with_set(v), v === "edock" ? "docker build now builds on eDock" : "docker build follows docker's context again");
    if (b) buildWith = b;
  }

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
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
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

  // ── The engine's log ──
  let logOpen = $state(false);
  let log = $state("");
  let logErrors = $state(false);
  async function loadLog() {
    log = (await attempt(() => api.engine_log(500, logErrors))) ?? "";
  }
  $effect(() => {
    logErrors;
    if (logOpen) loadLog();
  });

  // ── Speed test ──
  let bench = $state(null);
  let benching = $state(null);
  const offBench = channel("bench:progress").subscribe((p) => (benching = p));
  onDestroy(offBench);
  async function runBench() {
    benching = { engine: "", done: 0, of: 20 };
    const rows = await attempt(() => api.speed_test(20));
    benching = null;
    if (rows) bench = rows;
  }
  const edockP50 = $derived(bench?.find((r) => r.engine === "eDock" && !r.error)?.p50);
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

{#if buildWith}
  <Card title="docker build uses">
    <div class="engine-card">
      <div class="port-form">
        <Select
          value={buildWith.with}
          options={[
            { value: "docker", label: "docker's context" },
            { value: "edock", label: "eDock, always" },
          ]}
          onchange={setBuildWith}
        />
      </div>
      <p class="muted text">
        {#if buildWith.with === "edock"}
          <code>docker build</code>, <code>docker compose build</code> and <code>docker buildx build</code> run on eDock's BuildKit,
          whatever context docker uses, and the images land in eDock{buildWith.engine ? `, not in ${buildWith.engine}` : ""}.
          Everything else still follows the context.
        {:else}
          Builds follow docker's context: on eDock when docker uses eDock{buildWith.engine ? `, on ${buildWith.engine} while it uses ${buildWith.engine}` : ""}.
          <code>edock build</code> always builds on eDock.
        {/if}
      </p>
      {#if buildWith.with === "edock" && !buildWith.ready}
        <Callout variant="info">It takes effect once eDock's builder is made: <strong>Set up</strong> on the Overview tab makes it.</Callout>
      {/if}
    </div>
  </Card>
{/if}

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

<Card title="Speed test">
  <div class="engine-card">
    <p class="muted text">
      How long a container takes on this Mac, from create to removed (<code>alpine</code> running <code>true</code>, no network), on
      eDock and on any other engine running here. 20 runs each, after a few to warm up. Each container is removed straight away.
    </p>
    {#if benching}
      <p class="muted">{benching.engine ? `${benching.engine}: ${benching.done} of ${benching.of}…` : "Starting…"}</p>
    {:else if bench}
      <table class="kv bench">
        <thead><tr><td></td><td>median</td><td>slowest 10%</td><td></td></tr></thead>
        <tbody>
          {#each bench as r (r.engine)}
            <tr>
              <td>{r.engine}</td>
              {#if r.error}
                <td colspan="3" class="muted">{r.error}</td>
              {:else}
                <td class="mono">{r.p50.toFixed(1)} ms</td>
                <td class="mono">{r.p90.toFixed(1)} ms</td>
                <td class="muted">{r.engine !== "eDock" && edockP50 ? `${(r.p50 / edockP50).toFixed(1)}× eDock` : ""}</td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
  {#snippet actions()}<Button size="sm" loading={!!benching} onclick={runBench}>{bench ? "Run again" : "Run"}</Button>{/snippet}
</Card>

<Card title="Diagnostics">
  <p class="muted text">A report of eDock's state and recent logs, with tokens taken out, for when something's wrong. It's saved where you choose; nothing is sent.</p>
  {#snippet actions()}
    <Button size="sm" variant="ghost" onclick={() => (logOpen = true)}>Engine log</Button>
    <Button size="sm" onclick={report}>Save report…</Button>
  {/snippet}
</Card>

<Modal bind:open={logOpen} title="Engine log" description="eDock's engine, its last 500 lines, oldest first." size="lg">
  <div class="logs-bar">
    <label class="toggle"><Switch bind:checked={logErrors} /> Warnings and errors only</label>
    <span class="spacer"></span>
    <Button size="sm" variant="ghost" onclick={loadLog}>Refresh</Button>
    <Button size="sm" variant="ghost" onclick={() => clipboard.writeText(log).then(() => toast("Copied"))}>Copy</Button>
  </div>
  <pre class="raw engine-log">{log || "Nothing yet."}</pre>
</Modal>
