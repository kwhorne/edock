<script>
  import { navigate, clipboard, toast, confirm, prompt } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton, Tabs } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import Logs from "../lib/Logs.svelte";
  import Stats from "../lib/Stats.svelte";
  import Terminal from "../lib/Terminal.svelte";
  import Ports from "../lib/Ports.svelte";
  import Files from "../lib/Files.svelte";
  import { Modal } from "@elyra/ui";
  import { api } from "../bindings";
  import { changes, engine, attempt, errorText } from "../lib/state.svelte.js";
  import { stateTone, shortId } from "../lib/format.js";

  let { params, query } = $props();

  let detail = $state(null);
  let error = $state(null);
  let tab = $state("logs");
  // ?tab=terminal and the like, also when it changes on this page.
  $effect(() => {
    if (query?.tab) tab = query.tab;
  });
  let busy = $state(null);

  async function load() {
    try {
      detail = await api.container_inspect(params.id);
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    params.id;
    changes.container;
    load();
  });

  const running = $derived(detail?.state === "running");

  async function act(action) {
    busy = action;
    await attempt(() => api.container_action(params.id, action));
    busy = null;
    if (action === "remove") navigate("/");
  }

  async function remove() {
    if (await confirm(`Remove ${detail.name}?`, { danger: true, confirmLabel: "Remove" })) act("remove");
  }

  // Its files as they are now, to go back to (Snapshots page).
  async function snapshot() {
    const note = await prompt(`Snapshot ${detail.name}'s files. A note, if you like:`, { placeholder: "before the upgrade", confirmLabel: "Take snapshot" });
    if (note == null) return;
    busy = "snapshot";
    const image = await attempt(() => api.container_snapshot(params.id, note.trim() || null));
    busy = null;
    if (image) toast(`Snapshot taken: ${image}. Its volumes aren't in it.`, { variant: "success", duration: 6000 });
  }

  const tabs = $derived([
    { id: "logs", label: "Logs" },
    { id: "stats", label: "Stats", disabled: !running },
    { id: "terminal", label: "Terminal", disabled: !running },
    { id: "files", label: "Files" },
    { id: "ports", label: "Ports" },
    { id: "inspect", label: "Inspect" },
  ]);

  // The environment, one row per variable: a value is kept exactly as it
  // is (several lines, a PEM key; spaces at the ends). A running container
  // restarts with it, its files kept (eDock's engine).
  let envOpen = $state(false);
  let envRows = $state([]);
  let envBusy = $state(false);
  function openEnv() {
    envRows = detail.env.map((e) => ({ key: e.key, value: e.value }));
    envOpen = true;
  }
  async function saveEnv() {
    const rows = envRows.filter((r) => r.key.trim() !== "" || r.value !== "");
    const bad = rows.find((r) => !/^[^=\s]+$/.test(r.key.trim()));
    if (bad) return toast(`“${bad.key}” isn't a variable name: no spaces or =`, { variant: "error" });
    const env = rows.map((r) => `${r.key.trim()}=${r.value}`);
    if (running && !(await confirm(`${detail.name} restarts with the new environment. Its files are kept.`, { confirmLabel: "Save and restart" }))) return;
    envBusy = true;
    const ok = await attempt(() => api.container_set_env(params.id, env).then(() => true), "Environment saved");
    envBusy = false;
    if (ok) {
      envOpen = false;
      load();
    }
  }
</script>

<Modal bind:open={envOpen} title="Environment" description="Values are kept exactly as typed, several lines too." size="lg">
  <div class="env-rows">
    {#each envRows as row, i (i)}
      <div class="env-row">
        <input class="mono" bind:value={row.key} placeholder="NAME" aria-label="Name" />
        <textarea class="mono" bind:value={row.value} rows={Math.min(6, (row.value.match(/\n/g)?.length ?? 0) + 1)} aria-label="Value of {row.key}"></textarea>
        <button class="icon-btn danger" title="Remove" onclick={() => envRows.splice(i, 1)}><Icon name="trash" size={14} /></button>
      </div>
    {/each}
    <Button size="sm" onclick={() => envRows.push({ key: "", value: "" })}><Icon name="plus" size={13} /> Add a variable</Button>
  </div>
  {#snippet footer()}
    <Button onclick={() => (envOpen = false)}>Cancel</Button>
    <Button variant="primary" loading={envBusy} onclick={saveEnv}>Save</Button>
  {/snippet}
</Modal>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/")}><Icon name="back" /></button>
    {#if detail}
      <h1>{detail.name}</h1>
      <Badge variant={detail.exit_code ? "danger" : stateTone(detail.state)} title={detail.exit_code != null ? `Its program exited with code ${detail.exit_code}` : undefined}>{detail.state}{detail.exit_code != null ? ` (${detail.exit_code})` : ""}</Badge>
      <span class="muted mono" title={detail.id}>{shortId(detail.id)}</span>
      <span class="spacer"></span>
      {#if running}
        <Button size="sm" loading={busy === "stop"} onclick={() => act("stop")}><Icon name="stop" size={14} /> Stop</Button>
      {:else}
        <Button size="sm" variant="primary" loading={busy === "start"} onclick={() => act("start")}><Icon name="play" size={14} /> Start</Button>
      {/if}
      <Button size="sm" loading={busy === "restart"} onclick={() => act("restart")}><Icon name="restart" size={14} /> Restart</Button>
      <Button size="sm" loading={busy === "snapshot"} onclick={snapshot} title="Snapshot its files"><Icon name="camera" size={14} /> Snapshot</Button>
      <Button size="sm" variant="danger" onclick={remove} title="Remove" aria-label="Remove"><Icon name="trash" size={14} /></Button>
    {:else}
      <h1>Container</h1>
    {/if}
  </header>

  {#if error}
    <Callout variant="warning" title="Couldn't read the container">{error}</Callout>
  {:else if !detail}
    <Skeleton lines={8} />
  {:else}
    <div class="facts">
      <div><span class="muted">Image</span>{detail.image}</div>
      <div><span class="muted">Command</span><code>{[detail.entrypoint, detail.cmd].filter(Boolean).join(" ") || "—"}</code></div>
      {#if detail.ports.length}
        <div><span class="muted">Ports</span>{detail.ports.map((p) => (p.value ? `${p.value} → ${p.key}` : p.key)).join(", ")}</div>
      {/if}
      {#if detail.networks.length}
        <div><span class="muted">Networks</span>{detail.networks.map((n) => `${n.key}${n.value ? ` (${n.value})` : ""}`).join(", ")}</div>
      {/if}
    </div>

    <Tabs {tabs} bind:value={tab} label="Container">
      {#snippet panel(id)}
        {#if id === "logs"}
          <Logs container={params.id} />
        {:else if id === "stats"}
          <Stats container={params.id} />
        {:else if id === "terminal"}
          <Terminal container={params.id} />
        {:else if id === "files"}
          <Files container={params.id} {running} />
        {:else if id === "ports"}
          <Ports id={params.id} row={{ state: detail.state, domain: detail.domain }} />
        {:else if id === "inspect"}
          <div class="inspect">
            <section>
              <h3>
                Environment
                {#if engine.status?.context === "edock"}
                  <button class="ghost-btn" onclick={openEnv}>Edit</button>
                {/if}
              </h3>
              <table class="kv">
                <tbody>
                  {#each detail.env as e, i (`${e.key}:${i}`)}<tr><td>{e.key}</td><td class="mono">{e.value}</td></tr>{/each}
                </tbody>
              </table>
            </section>
            {#if detail.mounts.length}
              <section>
                <h3>Mounts</h3>
                <table class="kv">
                  <tbody>
                    {#each detail.mounts as m (m.destination)}
                      <tr><td class="mono">{m.destination}</td><td class="mono">{m.kind}: {m.source}{m.rw ? "" : " (read-only)"}</td></tr>
                    {/each}
                  </tbody>
                </table>
              </section>
            {/if}
            {#if detail.labels.length}
              <section>
                <h3>Labels</h3>
                <table class="kv">
                  <tbody>
                    {#each detail.labels as l (l.key)}<tr><td class="mono">{l.key}</td><td class="mono">{l.value}</td></tr>{/each}
                  </tbody>
                </table>
              </section>
            {/if}
            <section>
              <h3>
                docker inspect
                <button class="icon-btn" title="Copy" onclick={() => clipboard.writeText(detail.raw).then(() => toast("Copied"))}><Icon name="copy" size={14} /></button>
              </h3>
              <pre class="raw">{detail.raw}</pre>
            </section>
          </div>
        {/if}
      {/snippet}
    </Tabs>
  {/if}
</div>
