<!-- One Linux machine: a login shell as your user, in your home. -->
<script>
  import { navigate, confirm } from "@elyra/runtime";
  import { Badge, Button, Callout } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import Terminal from "../lib/Terminal.svelte";
  import { api } from "../bindings";
  import { changes, attempt } from "../lib/state.svelte.js";
  import { stateTone } from "../lib/format.js";

  let { params } = $props();
  let machine = $state(null);
  let error = $state(null);
  let busy = $state(null);

  async function load() {
    try {
      machine = (await api.machines_list()).find((m) => m.name === params.name) ?? null;
      error = machine ? null : "No such machine";
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }
  $effect(() => {
    params.name;
    changes.container;
    load();
  });

  async function act(action) {
    if (action === "delete" && !(await confirm(`Delete ${params.name}? Your files in /Users stay.`, { danger: true, confirmLabel: "Delete" }))) return;
    busy = action;
    await attempt(() => api.machine_action(params.name, action));
    busy = null;
    if (action === "delete") navigate("/machines");
  }

  const running = $derived(machine?.state === "running");
</script>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/machines")}><Icon name="back" /></button>
    <h1>{params.name}</h1>
    {#if machine}
      <Badge variant={stateTone(machine.state)}>{machine.state}</Badge>
      <span class="muted">{machine.distro}{machine.systemd ? " · systemd" : ""}</span>
    {/if}
    <span class="spacer"></span>
    {#if machine}
      {#if running}
        <Button size="sm" loading={busy === "stop"} onclick={() => act("stop")}><Icon name="stop" size={14} /> Stop</Button>
      {:else}
        <Button size="sm" variant="primary" loading={busy === "start"} onclick={() => act("start")}><Icon name="play" size={14} /> Start</Button>
      {/if}
      <Button size="sm" loading={busy === "restart"} onclick={() => act("restart")}><Icon name="restart" size={14} /> Restart</Button>
      <Button size="sm" variant="danger" onclick={() => act("delete")}><Icon name="trash" size={14} /></Button>
    {/if}
  </header>

  {#if error}
    <Callout variant="warning" title="Couldn't open the machine">{error}</Callout>
  {:else if machine && !running}
    <Callout variant="info" title="{params.name} is stopped">Start it to open a shell.</Callout>
  {:else if machine}
    <p class="muted">A login shell as your user, in your home. From the terminal: <code>edock machine shell {params.name}</code></p>
    {#key params.name}
      <div class="machine-term"><Terminal start={(cols, rows) => api.machine_shell(params.name, cols, rows)} /></div>
    {/key}
  {/if}
</div>
