<script>
  import { channel, confirm, toast } from "@elyra/runtime";
  import { onDestroy } from "svelte";
  import { Checkbox } from "@elyra/ui";
  import { checkForUpdates } from "../lib/updates.js";
  import { Badge, Button, Callout, Card, Switch, Tabs } from "@elyra/ui";
  import EngineResources from "../lib/EngineResources.svelte";
  import EngineDomains from "../lib/EngineDomains.svelte";
  import EngineBuilds from "../lib/EngineBuilds.svelte";
  import EngineProxy from "../lib/EngineProxy.svelte";
  import EngineUsb from "../lib/EngineUsb.svelte";
  import { attempt as tryIt } from "../lib/state.svelte.js";

  // macOS notes when a container runs out of memory or turns unhealthy.
  let alerts = $state(null);
  api.alerts_on().then((on) => (alerts = on));
  const setAlerts = (on) => tryIt(() => api.alerts_set(on)).then(() => (alerts = on));
  import EngineAgents from "../lib/EngineAgents.svelte";
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
    const ok = await confirm("Stop eDock's engine? Its containers stop, and nothing runs until you start it again.", { confirmLabel: "Stop" });
    if (!ok) return;
    busy = "stop";
    await attempt(() => api.daemon_stop(), "eDock engine stopped");
    busy = null;
    await Promise.all([loadDaemon(), refreshEngine()]);
  }

  const s = $derived(engine.status);

  // The command line: `edock setup`.
  let cli = $state(null);
  let useAsCurrent = $state(true);
  let setupLines = $state([]);
  async function loadCli() {
    cli = await api.cli_status();
  }
  $effect(() => {
    changes.engine;
    loadCli();
  });
  const offSetup = channel("setup:progress").subscribe((p) => {
    if (!p) return;
    if (p.line) setupLines = [...setupLines.slice(-30), p.line];
    if (p.done) {
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
      else toast("docker and edock are set up. Open a new terminal to use them.", { variant: "success" });
      loadCli();
      refreshEngine();
    }
  });
  onDestroy(offSetup);
  async function setupCli() {
    setupLines = [];
    await attempt(() => api.cli_setup(useAsCurrent));
    await loadCli();
  }
  const cliReady = $derived(cli?.docker && cli?.context);
  // Off: no daemon. Stopped: turned off with Stop. Asleep: on, its VM idle.
  const STATE = {
    running: ["success", "running"],
    asleep: ["accent", "running · VM asleep"],
    stopped: ["warning", "stopped"],
    off: ["neutral", "not running"],
  };
  const on = $derived(daemon?.state === "running" || daemon?.state === "asleep");
  let { query } = $props();
  let tab = $state("overview");
  $effect(() => {
    if (query?.tab) tab = query.tab;
  });
  const edockOn = $derived(s?.context === "edock" && s?.connected);

</script>

<div class="page narrow">
  <header class="page-head"><h1>Engine</h1></header>

  <Tabs
    tabs={[
      { id: "overview", label: "Overview" },
      { id: "resources", label: "Resources", disabled: !edockOn },
      { id: "domains", label: "Network", disabled: !edockOn },
      { id: "builds", label: "Builds & import", disabled: !edockOn },
      { id: "usb", label: "USB", disabled: !edockOn },
      { id: "agents", label: "AI agents" },
    ]}
    bind:value={tab}
    label="Engine"
  >
    {#snippet panel(id)}
      {#if id === "resources"}
        <div class="tab-cards"><EngineResources /></div>
      {:else if id === "domains"}
        <div class="tab-cards"><EngineDomains /><EngineProxy /></div>
      {:else if id === "builds"}
        <div class="tab-cards"><EngineBuilds /></div>
      {:else if id === "usb"}
        <div class="tab-cards"><EngineUsb /></div>
      {:else if id === "agents"}
        <div class="tab-cards"><EngineAgents /></div>
      {:else}
      <div class="tab-cards">

  <Card title="eDock engine">
    <div class="engine-card">
    {#if daemon}
      <p>
        <Badge variant={STATE[daemon.state][0]}>{STATE[daemon.state][1]}</Badge>
        <span class="muted mono">~/.edock/run/docker.sock</span>
      </p>
      {#if daemon.state === "asleep"}
        <p class="muted">The VM sleeps while nothing needs it, and wakes in a fraction of a second.</p>
      {:else if daemon.state === "stopped"}
        <p class="muted">Stopped: containers don't run until you start it again.</p>
      {/if}
      {#if daemon.binary}
        <p class="muted">Engine: <span class="mono">{daemon.binary}</span></p>
      {:else}
        <Callout variant="info" title="No engine binary found">
          Build it with <code>cargo xtask host</code> in <code>engine/</code>, put it in
          <code>~/.edock/bin/edock</code>, or set <code>EDOCK_ENGINE</code>.
        </Callout>
      {/if}
    {/if}
    </div>
    {#snippet actions()}
      {#if on}
        <Button size="sm" loading={busy === "stop"} onclick={stop}>Stop</Button>
      {:else if daemon}
        <!-- A stopped engine runs already: Start needs no binary for it. -->
        <Button size="sm" variant="primary" loading={busy === "start"} disabled={daemon.state === "off" && !daemon.binary} onclick={start}>Start</Button>
      {/if}
    {/snippet}
  </Card>

  <Card title="Command line">
    <div class="engine-card">
      {#if cli?.stale}
        <Callout variant="warning" title="Terminals run another edock">
          <span class="mono">{cli.stale}</span> comes first on your PATH, so terminals run it instead of this eDock's: an older copy, most
          likely. Remove it, or put <span class="mono">~/.edock/bin</span> before its folder in your PATH.
        </Callout>
      {/if}
      {#if cli?.busy || setupLines.length}
        <pre class="progress">{setupLines.join("\n") || "…"}</pre>
      {/if}
      {#if cliReady}
        <p><Badge variant="success">set up</Badge> <code>docker</code> and <code>edock</code> are in <span class="mono">~/.edock/bin</span>, with the <code>edock</code> context.</p>
        {#if !cli.agent}<p class="muted">The engine doesn't start at login (no LaunchAgent).</p>{/if}
      {:else if cli && !cli.busy}
        <p class="muted text">
          Use eDock from a terminal: the <code>docker</code> CLI with Compose and buildx (downloaded once, checked
          against their published checksums), <code>edock</code>, an <code>edock</code> docker context, and the engine
          started at login. No package manager needed.
        </p>
        <Checkbox class="toggle" bind:checked={useAsCurrent}>Make <code>edock</code> docker's current context</Checkbox>
      {/if}
    </div>
    {#snippet actions()}
      {#if cli && !cliReady}
        <Button size="sm" variant="primary" loading={cli.busy} disabled={!daemon?.binary} onclick={setupCli}>Set up</Button>
      {:else if cliReady}
        <Button size="sm" loading={cli.busy} onclick={setupCli}>Set up again</Button>
      {/if}
    {/snippet}
  </Card>

  <Card title="Notifications">
    <div class="engine-card">
      <label class="toggle"><Switch checked={!!alerts} onchange={setAlerts} disabled={alerts === null} /> Tell me when a container runs out of memory or turns unhealthy, or containers keep waiting for memory</label>
      <p class="muted text">A macOS notification, once per container every ten minutes (for memory, once in half an hour); click it for the container's log, or this page. Not for eDock's own containers.</p>
    </div>
  </Card>

  <p class="version-row muted">
    eDock {s?.app_version ?? ""}
    <button class="ghost-btn" onclick={checkForUpdates}>Check for updates</button>
  </p>

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
          {#if s.state === "stopped"}
            <Badge variant="warning">stopped</Badge>
          {:else}
            <Badge variant={s.connected ? "success" : "warning"}>{s.connected ? "connected" : "not answering"}</Badge>
          {/if}
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
      {/if}
    {/snippet}
  </Tabs>
</div>
