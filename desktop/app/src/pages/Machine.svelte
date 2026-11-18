<!-- One Linux machine: a login shell as your user, in your home; its files,
     ports and stats; and `ssh NAME.edock` for editors that work over SSH. -->
<script>
  import { navigate, confirm, clipboard, shell, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Tabs } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import Terminal from "../lib/Terminal.svelte";
  import Files from "../lib/Files.svelte";
  import Ports from "../lib/Ports.svelte";
  import Stats from "../lib/Stats.svelte";
  import { api } from "../bindings";
  import { changes, attempt, errorText } from "../lib/state.svelte.js";
  import { stateTone } from "../lib/format.js";

  let { params, query } = $props();
  let machine = $state(null);
  let tab = $state("terminal");
  $effect(() => {
    if (query?.tab) tab = query.tab;
  });
  const id = $derived(`machine-${params.name}`);

  // ssh NAME.edock: on for every machine, or not yet.
  let ssh = $state(null);
  const loadSsh = () => api.machine_ssh().then((s) => (ssh = s), () => (ssh = null));
  loadSsh();
  async function sshOn() {
    const ok = await confirm(
      "eDock makes an SSH key of its own (in ~/.edock/ssh) and puts one line first in ~/.ssh/config: Include ~/.edock/ssh/config. Then ssh NAME.edock reaches every machine, through the engine, with no password. The first time, openssh-server is installed in the machine (it isn't left running).",
      { confirmLabel: "Set up SSH" },
    );
    if (!ok) return;
    if ((await attempt(() => api.machine_ssh_set(true).then(() => true))) === true) toast(`ssh ${params.name}.edock works now`, { variant: "success" });
    loadSsh();
  }
  async function sshOff() {
    if (!(await confirm("Take eDock's line out of ~/.ssh/config? ssh NAME.edock stops working for every machine.", { confirmLabel: "Turn off" }))) return;
    await attempt(() => api.machine_ssh_set(false));
    loadSsh();
  }
  const host = $derived(`${params.name}.edock`);
  function openIn(editor) {
    const scheme = editor === "Cursor" ? "cursor" : "vscode";
    shell.openExternal(`${scheme}://vscode-remote/ssh-remote+${host}${ssh.home}`);
  }
  let error = $state(null);
  let busy = $state(null);

  async function load() {
    try {
      machine = (await api.machines_list()).find((m) => m.name === params.name) ?? null;
      error = machine ? null : "No such machine";
    } catch (e) {
      error = errorText(e);
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
  // Terminal and Stats need it running: a stopped one shows its files.
  $effect(() => {
    if (machine && !running && (tab === "terminal" || tab === "stats")) tab = "files";
  });
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
  {:else if machine}
    {#if ssh}
      <div class="machine-ssh">
        {#if ssh.on}
          <span class="muted">SSH</span>
          <button class="mono link-path" title="Copy" onclick={() => clipboard.writeText(`ssh ${host}`).then(() => toast("Copied"))}>ssh {host}</button>
          {#each ssh.editors as e (e)}
            <Button size="sm" onclick={() => openIn(e)} title="Over SSH, with the Remote - SSH extension">Open in {e === "Visual Studio Code" ? "VS Code" : e}</Button>
          {/each}
          <span class="spacer"></span>
          <button class="link small" onclick={sshOff}>Turn off</button>
        {:else}
          <span class="muted">Reach it as <code>ssh {host}</code>, and open it in VS Code, Cursor or Zed over SSH.</span>
          <span class="spacer"></span>
          <Button size="sm" onclick={sshOn}>Set up SSH…</Button>
        {/if}
      </div>
    {/if}
    {#if !running}
      <Callout variant="info" title="{params.name} is stopped">Start it to open a shell{ssh?.on ? `, or ssh ${host} starts it` : ""}.</Callout>
    {/if}
    <Tabs
      tabs={[
        { id: "terminal", label: "Terminal", disabled: !running },
        { id: "files", label: "Files" },
        { id: "ports", label: "Ports" },
        { id: "stats", label: "Stats", disabled: !running },
      ]}
      bind:value={tab}
      label="Machine"
    >
      {#snippet panel(t)}
        {#if t === "terminal" && running}
          <div class="machine-shell">
            <p class="muted">A login shell as your user, in your home. From the terminal: <code>edock machine shell {params.name}</code></p>
            {#key params.name}
              <div class="machine-term"><Terminal start={(cols, rows) => api.machine_shell(params.name, cols, rows)} /></div>
            {/key}
          </div>
        {:else if t === "files"}
          {#key id}<Files container={id} {running} />{/key}
        {:else if t === "ports"}
          {#key id}<Ports id={id} row={{ state: machine.state, domain: null }} />{/key}
        {:else if t === "stats" && running}
          {#key id}<Stats container={id} />{/key}
        {/if}
      {/snippet}
    </Tabs>
  {/if}
</div>
