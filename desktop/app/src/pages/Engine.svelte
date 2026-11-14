<script>
  import { channel, confirm, toast } from "@elyra/runtime";
  import { onDestroy } from "svelte";
  import { Checkbox } from "@elyra/ui";
  import { checkForUpdates } from "../lib/updates.js";
  import { Badge, Button, Callout, Card, Tabs } from "@elyra/ui";
  import EngineResources from "../lib/EngineResources.svelte";
  import EngineDomains from "../lib/EngineDomains.svelte";
  import EngineBuilds from "../lib/EngineBuilds.svelte";
  import EngineProxy from "../lib/EngineProxy.svelte";
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

  // After an update the engine that ran keeps the old version until it
  // starts again (a checkout's engine says 0.0.0: no comparing then).
  const outdated = $derived(
    s?.context === "edock" && s?.connected && s.version && s.version !== s.app_version && s.version !== "0.0.0" && s.app_version !== "0.0.0",
  );

  async function restart() {
    busy = "restart";
    await attempt(() => api.daemon_restart(), `The engine runs ${s.app_version} now`);
    busy = null;
    await Promise.all([loadDaemon(), refreshEngine()]);
  }

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
      if (p.error) toast(p.error, { variant: "danger", duration: 8000 });
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

  {#if outdated}
    <Callout variant="warning" title="The engine runs an older eDock">
      eDock was updated to {s.app_version}, and the engine still runs {s.version}. Restart it to run the new one;
      running containers stop and start again.
      <div class="callout-action"><Button size="sm" variant="primary" loading={busy === "restart"} onclick={restart}>Restart engine</Button></div>
    </Callout>
  {/if}

  <Card title="Command line">
    <div class="engine-card">
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
        <label class="toggle"><Checkbox bind:checked={useAsCurrent} /> Make <code>edock</code> docker's current context</label>
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
