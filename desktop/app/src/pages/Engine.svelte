<script>
  import { Badge, Button, Callout, Card } from "@elyra/ui";
  import { api } from "../bindings";
  import { engine, changes, refreshEngine, useContext, attempt } from "../lib/state.svelte.js";
  import { bytes } from "../lib/format.js";

  let daemon = $state(null);
  let busy = $state(null);

  async function loadDaemon() {
    daemon = await api.daemon_info();
  }

  $effect(() => {
    changes.engine;
    loadDaemon();
  });

  async function start() {
    busy = "start";
    await attempt(() => api.daemon_start(), "eDock engine started");
    busy = null;
    await Promise.all([loadDaemon(), refreshEngine()]);
  }

  async function stop() {
    busy = "stop";
    await attempt(() => api.daemon_stop(), "eDock engine stopped");
    busy = null;
    await Promise.all([loadDaemon(), refreshEngine()]);
  }

  const s = $derived(engine.status);
</script>

<div class="page narrow">
  <header class="page-head"><h1>Engine</h1></header>

  <Card title="eDock engine">
    {#if daemon}
      <p>
        {#if daemon.running}
          <Badge variant="success">running</Badge>
        {:else}
          <Badge>stopped</Badge>
        {/if}
        <span class="muted mono">~/.edock/run/docker.sock</span>
      </p>
      {#if daemon.binary}
        <p class="muted">Engine: <span class="mono">{daemon.binary}</span></p>
      {:else}
        <Callout variant="info" title="No engine binary found">
          Build it with <code>cargo xtask host</code> in <code>engine/</code>, put it in
          <code>~/.edock/bin/edock</code>, or set <code>EDOCK_ENGINE</code>.
        </Callout>
      {/if}
    {/if}
    {#snippet actions()}
      {#if daemon?.running}
        <Button size="sm" loading={busy === "stop"} disabled={!daemon?.ours} onclick={stop}>Stop</Button>
      {:else}
        <Button size="sm" variant="primary" loading={busy === "start"} disabled={!daemon?.binary} onclick={start}>Start</Button>
      {/if}
    {/snippet}
  </Card>

  <h2>Contexts</h2>
  <p class="muted">
    The engine this window shows. eDock talks the Docker Engine API, so it also manages Docker Desktop, OrbStack,
    Colima or Podman.
  </p>
  <ul class="contexts">
    {#each engine.contexts as c (c.name)}
      <li class:current={s?.context === c.name}>
        <div>
          <strong>{c.name}</strong>
          {#if c.docker_current}<Badge variant="accent">docker's current</Badge>{/if}
          {#if !c.available}<Badge>no socket</Badge>{/if}
          <div class="muted mono">{c.host}</div>
          {#if c.description}<div class="muted">{c.description}</div>{/if}
        </div>
        {#if s?.context === c.name}
          <Badge variant={s.connected ? "success" : "warning"}>{s.connected ? "connected" : "not answering"}</Badge>
        {:else}
          <Button size="sm" onclick={() => useContext(c.name)}>Use</Button>
        {/if}
      </li>
    {/each}
  </ul>

  {#if s?.connected}
    <h2>{s.name || s.context}</h2>
    <dl class="facts-grid">
      <dt>Version</dt><dd>{s.version}</dd>
      <dt>OS</dt><dd>{s.os} ({s.arch})</dd>
      <dt>CPUs</dt><dd>{s.cpus}</dd>
      <dt>Memory</dt><dd>{s.memory > 0 ? bytes(s.memory) : "— (the VM is asleep)"}</dd>
      <dt>Containers</dt><dd>{s.running} running of {s.containers}</dd>
      <dt>Images</dt><dd>{s.images}</dd>
    </dl>
  {:else if s?.error}
    <Callout variant="warning" title="{s.context} isn't answering">{s.error}</Callout>
  {/if}
</div>
