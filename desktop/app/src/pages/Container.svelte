<script>
  import { navigate, clipboard, toast, confirm } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton, Tabs } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import Logs from "../lib/Logs.svelte";
  import Stats from "../lib/Stats.svelte";
  import Terminal from "../lib/Terminal.svelte";
  import { api } from "../bindings";
  import { changes, attempt } from "../lib/state.svelte.js";
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
      error = e?.message ?? String(e);
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

  const tabs = $derived([
    { id: "logs", label: "Logs" },
    { id: "stats", label: "Stats", disabled: !running },
    { id: "terminal", label: "Terminal", disabled: !running },
    { id: "inspect", label: "Inspect" },
  ]);
</script>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/")}><Icon name="back" /></button>
    {#if detail}
      <h1>{detail.name}</h1>
      <Badge variant={stateTone(detail.state)}>{detail.state}</Badge>
      <span class="muted mono" title={detail.id}>{shortId(detail.id)}</span>
      <span class="spacer"></span>
      {#if running}
        <Button size="sm" loading={busy === "stop"} onclick={() => act("stop")}><Icon name="stop" size={14} /> Stop</Button>
      {:else}
        <Button size="sm" variant="primary" loading={busy === "start"} onclick={() => act("start")}><Icon name="play" size={14} /> Start</Button>
      {/if}
      <Button size="sm" loading={busy === "restart"} onclick={() => act("restart")}><Icon name="restart" size={14} /> Restart</Button>
      <Button size="sm" variant="danger" onclick={remove}><Icon name="trash" size={14} /></Button>
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
        {:else if id === "inspect"}
          <div class="inspect">
            <section>
              <h3>Environment</h3>
              <table class="kv">
                <tbody>
                  {#each detail.env as e (e.key)}<tr><td>{e.key}</td><td class="mono">{e.value}</td></tr>{/each}
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
