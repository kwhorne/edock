<!-- The first time eDock opens: the engine, the terminal, what to bring
     over, and HTTPS, each a click, all skippable. -->
<script>
  import { channel, navigate, toast } from "@elyra/runtime";
  import { Badge, Button, Card, Checkbox } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { engine, refreshEngine, useContext, attempt } from "../lib/state.svelte.js";

  let daemon = $state(null);
  let cli = $state(null);
  let sources = $state([]);
  let busy = $state(null);
  let useAsCurrent = $state(true);
  let lines = $state([]);

  async function load() {
    [daemon, cli, sources] = await Promise.all([api.daemon_info(), api.cli_status(), api.import_sources()]);
  }
  load();

  const engineOn = $derived(daemon && (daemon.state === "running" || daemon.state === "asleep"));
  const cliReady = $derived(cli?.docker && cli?.context);
  const other = $derived(sources.find((s) => s.available));

  async function start() {
    busy = "engine";
    await attempt(() => api.daemon_start());
    busy = null;
    await Promise.all([load(), refreshEngine()]);
  }
  async function setup() {
    busy = "cli";
    lines = [];
    await attempt(() => api.cli_setup(useAsCurrent));
  }
  const off = channel("setup:progress").subscribe((p) => {
    if (!p) return;
    if (p.line) lines = [...lines.slice(-6), p.line];
    if (p.done) {
      busy = null;
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
      load();
    }
  });
  onDestroy(off);
  // Steps 3 and 4 are about eDock's engine: be on it first (the welcome may
  // open from ⌘K while the window shows another engine).
  async function onEdock() {
    if (engine.status?.context !== "edock") await useContext("edock");
  }
  async function importStep() {
    await onEdock();
    finish("/engine?tab=builds");
  }
  async function trust() {
    await onEdock();
    busy = "tls";
    await attempt(() => api.tls_trust(true), "HTTPS trusted");
    busy = null;
    engine.domains = await api.domains_info();
  }
  async function finish(path = "/") {
    await api.welcome_done();
    navigate(path);
  }
</script>

<div class="page narrow welcome">
  <header class="welcome-head">
    <img src="./icon.svg" alt="" width="56" height="56" />
    <div>
      <h1>Welcome to eDock</h1>
      <p class="muted">A few clicks to get going. Each step can wait, and every one is on the Engine page later.</p>
    </div>
  </header>

  <Card title="1 · Start the engine">
    <p class="muted text">eDock's own engine: containers start in milliseconds, and its VM sleeps when nothing runs.</p>
    {#snippet actions()}
      {#if engineOn}<Badge variant="success">running</Badge>
      {:else}<Button size="sm" variant="primary" loading={busy === "engine"} disabled={!daemon?.binary} onclick={start}>Start</Button>{/if}
    {/snippet}
  </Card>

  <Card title="2 · Use it from the terminal">
    <div class="engine-card">
      <p class="muted text">
        The <code>docker</code> CLI with Compose and buildx (no package manager needed), <code>edock</code>, an
        <code>edock</code> docker context, and the engine started at login.
      </p>
      {#if !cliReady}<label class="toggle"><Checkbox bind:checked={useAsCurrent} /> Make <code>edock</code> docker's current context</label>{/if}
      {#if lines.length}<pre class="progress">{lines.join("\n")}</pre>{/if}
    </div>
    {#snippet actions()}
      {#if cliReady}<Badge variant="success">set up</Badge>
      {:else}<Button size="sm" variant="primary" loading={busy === "cli"} disabled={!engineOn} onclick={setup}>Set up</Button>{/if}
    {/snippet}
  </Card>

  {#if other}
    <Card title="3 · Bring things over from {other.label}">
      <p class="muted text">Your images, volumes and containers, copied into eDock. {other.label} is only read.</p>
      {#snippet actions()}
        <Button size="sm" disabled={!engineOn} onclick={importStep}>Choose what to import</Button>
      {/snippet}
    </Card>
  {/if}

  <Card title="{other ? 4 : 3} · HTTPS with a padlock">
    <p class="muted text">
      Containers answer at <span class="mono">https://&lt;name&gt;.edock.localhost</span>. Trust eDock's certificates so
      browsers show a padlock: macOS asks for your password once.
    </p>
    {#snippet actions()}
      {#if engine.domains?.trusted}<Badge variant="success">trusted</Badge>
      {:else}<Button size="sm" loading={busy === "tls"} disabled={!engineOn} onclick={trust}>Trust HTTPS</Button>{/if}
    {/snippet}
  </Card>

  <div class="welcome-foot">
    <button class="ghost-btn" onclick={() => finish()}>Skip</button>
    <Button variant="primary" onclick={() => finish()}><Icon name="box" size={14} /> Done</Button>
  </div>
</div>
