<!-- Linux machines: long-lived Linux environments with your user and /Users. -->
<script>
  import { channel, confirm, navigate, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Checkbox, Empty, Input, Select, Skeleton } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, useContext, attempt } from "../lib/state.svelte.js";
  import { ago, stateTone } from "../lib/format.js";

  let machines = $state(null);
  let error = $state(null);
  let distros = $state([]);
  let form = $state({ name: "", distro: "ubuntu", systemd: false });
  let creating = $state(false);
  let lines = $state([]);
  let busy = $state({});

  const onEdock = $derived(engine.status?.context === "edock");

  async function load() {
    if (!onEdock) return;
    try {
      machines = await api.machines_list();
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  $effect(() => {
    changes.container;
    onEdock;
    load();
  });
  api.machine_distros().then((d) => (distros = d));

  const off = channel("machine:progress").subscribe((p) => {
    if (!p) return;
    if (p.line) lines = [...lines.slice(-30), p.line];
    if (p.done) {
      creating = false;
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
      else {
        toast(`${made} is ready`, { variant: "success" });
        navigate(`/machines/${made}`);
      }
      load();
    }
  });
  onDestroy(off);

  let made = "";
  async function create(e) {
    e.preventDefault();
    const name = form.name.trim();
    if (!/^[A-Za-z0-9][A-Za-z0-9-]*$/.test(name)) return toast("A name is letters, digits and dashes", { variant: "error" });
    lines = [];
    made = name;
    creating = true;
    const ok = await attempt(() => api.machine_create(name, form.distro, form.systemd).then(() => true));
    if (!ok) creating = false;
    else form.name = "";
  }

  async function act(m, action) {
    if (action === "delete" && !(await confirm(`Delete ${m.name}? What's only in the machine goes with it; your files in /Users stay.`, { danger: true, confirmLabel: "Delete" }))) return;
    busy[m.name] = action;
    await attempt(() => api.machine_action(m.name, action));
    busy[m.name] = null;
    load();
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Machines</h1>
    <span class="muted">{machines ? `${machines.filter((m) => m.state === "running").length} running` : ""}</span>
  </header>

  {#if !onEdock}
    <Callout variant="info" title="Linux machines run on eDock's engine">
      This window shows <strong>{engine.status?.context}</strong>.
      <div class="callout-action"><Button size="sm" onclick={() => useContext("edock")}>Switch to eDock</Button></div>
    </Callout>
  {:else}
    <Card title="New machine">
      <div class="engine-card">
      <form class="port-form" onsubmit={create}>
        <label>Name <Input bind:value={form.name} placeholder="dev" /></label>
        <label>Distribution
          <Select value={form.distro} options={distros.map((d) => ({ value: d.id, label: d.label }))} onchange={(v) => (form.distro = v)} />
        </label>
        <label class="toggle"><Checkbox bind:checked={form.systemd} /> systemd</label>
        <Button type="submit" size="sm" variant="primary" loading={creating}>Create</Button>
      </form>
      <p class="muted text">
        Your Mac user with passwordless <code>sudo</code>, and <code>/Users</code> at the same path, so your projects are
        where you expect them. systemd runs services as on a real server (it makes the machine privileged).
      </p>
      {#if creating || lines.length}<pre class="progress">{lines.join("\n") || "…"}</pre>{/if}
      </div>
    </Card>

    {#if error}
      <Callout variant="warning" title="Couldn't list the machines">{error}</Callout>
    {:else if machines === null}
      <Skeleton lines={3} />
    {:else if machines.length === 0}
      <Empty title="No machines yet" description="Make one above: a full Linux, ready in seconds, sharing your files." />
    {:else}
      <ul class="rows machine-list">
        {#each machines as m (m.name)}
          {@const running = m.state === "running"}
          <li>
            <div class="row" role="button" tabindex="0" onclick={() => navigate(`/machines/${m.name}`)} onkeydown={(e) => e.key === "Enter" && navigate(`/machines/${m.name}`)}>
              <span class="dot" class:on={running}></span>
              <div class="main">
                <div class="name">{m.name}</div>
                <div class="sub muted">{m.distro}{m.systemd ? " · systemd" : ""}</div>
              </div>
              <div></div>
              <div class="state">
                <Badge variant={stateTone(m.state)}>{m.state}</Badge>
                <div class="sub muted">{running ? m.status : `made ${ago(m.created)}`}</div>
              </div>
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="row-actions" onclick={(e) => e.stopPropagation()}>
                {#if running}
                  <button class="icon-btn" title="Shell" onclick={() => navigate(`/machines/${m.name}`)}><Icon name="terminal" size={14} /></button>
                  <button class="icon-btn" title="Stop" disabled={!!busy[m.name]} onclick={() => act(m, "stop")}><Icon name="stop" size={14} /></button>
                {:else}
                  <button class="icon-btn" title="Start" disabled={!!busy[m.name]} onclick={() => act(m, "start")}><Icon name="play" size={14} /></button>
                {/if}
                <button class="icon-btn" title="Restart" disabled={!!busy[m.name]} onclick={() => act(m, "restart")}><Icon name="restart" size={14} /></button>
                <button class="icon-btn danger" title="Delete" disabled={!!busy[m.name]} onclick={() => act(m, "delete")}><Icon name="trash" size={14} /></button>
              </div>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>
