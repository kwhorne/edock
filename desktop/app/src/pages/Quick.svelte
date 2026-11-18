<!-- The menu bar item's popover: the engine and its VM, what needs a look,
     and what runs (Compose projects as one, containers, Linux machines),
     a click away. -->
<script>
  import { quit, shell } from "@elyra/runtime";
  import { Button } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, domainUrl } from "../lib/state.svelte.js";
  import { bytes } from "../lib/format.js";

  let containers = $state([]);
  let projects = $state([]);
  let machines = $state([]);
  let vm = $state(null);
  let alerts = $state([]);
  let busy = $state({});

  const s = $derived(engine.status);
  const onEdock = $derived(s?.context === "edock");
  const up = $derived(s?.connected && s?.state !== "stopped");

  async function load() {
    const [c, p, m, a] = await Promise.all([
      api.containers_list(false).catch(() => []),
      api.compose_projects().catch(() => []),
      onEdock ? api.machines_list().catch(() => []) : [],
      api.alerts_recent().catch(() => []),
    ]);
    containers = c;
    projects = p;
    machines = m;
    alerts = a;
    vm = onEdock && up ? await api.vm_info().catch(() => null) : null;
  }

  // While it's open. Hidden, it loads when opened (`shown`): no looks at
  // the engine for nobody, which would also keep its VM from sleeping.
  $effect(() => {
    changes.container;
    s?.context;
    s?.state;
    if (!document.hidden) load();
  });

  // Opened from the menu bar: fresh, and the menu bar item loses its dot.
  function shown() {
    load();
    api.alerts_seen(false).catch(() => {});
  }
  window.addEventListener("focus", shown);
  onDestroy(() => window.removeEventListener("focus", shown));
  shown();

  const mine = $derived(containers.filter((c) => !c.system));
  const running = $derived(mine.filter((c) => c.state === "running"));
  const unhealthy = (c) => c.health === "unhealthy";

  // Compose projects as one row each, those that are down too.
  const groups = $derived.by(() => {
    const by = new Map();
    for (const c of mine.filter((c) => c.project)) {
      if (!by.has(c.project)) by.set(c.project, []);
      by.get(c.project).push(c);
    }
    const out = [...by].map(([name, items]) => ({
      name,
      total: items.length,
      running: items.filter((c) => c.state === "running").length,
      sick: items.some(unhealthy),
    }));
    for (const p of projects) if (!by.has(p.name)) out.push({ name: p.name, total: 0, running: 0, down: true, problem: p.problem });
    // Running first, then by name.
    return out.sort((a, b) => (b.running > 0) - (a.running > 0) || a.name.localeCompare(b.name));
  });
  const singles = $derived.by(() => {
    const own = mine.filter((c) => !c.project);
    return [...own.filter((c) => c.state === "running"), ...own.filter((c) => c.state !== "running").slice(0, 4)];
  });

  const open = (path) => api.window_open(path ?? null);

  async function projectDo(g, action) {
    busy[g.name] = true;
    if (action === "up") await attempt(() => api.project_up(g.name, false), `${g.name} is up`);
    else await attempt(() => api.project_action(g.name, action));
    busy[g.name] = false;
  }
  async function machineStart(m) {
    busy[`m:${m.name}`] = true;
    await attempt(() => api.machine_action(m.name, "start"));
    busy[`m:${m.name}`] = false;
    load();
  }

  const ago = (secs) => (secs < 60 ? "just now" : secs < 3600 ? `${Math.round(secs / 60)} min ago` : "an hour ago");
  async function clearAlerts() {
    await api.alerts_seen(true).catch(() => {});
    alerts = [];
  }
</script>

<div class="quick">
  <header class="quick-head">
    <span class="dot" class:on={up}></span>
    <strong>eDock</strong>
    <span class="muted">{s?.state === "stopped" ? `${s.context} stopped` : s?.connected ? `${s.context} · ${running.length} running` : `${s?.context ?? ""} not running`}</span>
  </header>

  {#if vm}
    <div class="quick-vm muted" title="The VM sleeps when nothing runs, and wakes on the next command">
      {#if vm.awake}
        VM awake · {bytes(vm.memory_used)} in use{vm.memory_ceiling ? ` of ${bytes(vm.memory_ceiling)}` : ""}
        {#if vm.memory_pressure !== "none"}
          <button class="link-btn" class:bad={vm.memory_pressure === "high"} onclick={() => open("/engine")}>· {vm.memory_pressure === "high" ? "containers wait on memory" : "memory is tight"}</button>
        {/if}
      {:else}
        VM asleep · it wakes on the next command
      {/if}
    </div>
  {/if}

  {#if onEdock && (!s?.connected || s?.state === "stopped")}
    <div class="quick-empty">
      <Button size="sm" variant="primary" onclick={() => attempt(() => api.daemon_start(), "eDock engine started")}>Start engine</Button>
    </div>
  {/if}

  {#if alerts.length}
    <div class="quick-alerts">
      <div class="quick-section"><span>Needs a look</span><button class="link small" onclick={clearAlerts}>Clear</button></div>
      {#each alerts as a (`${a.id}:${a.what}`)}
        <button class="quick-alert" onclick={() => open(a.what === "memory" ? "/engine" : `/containers/${a.id}?tab=logs`)}>
          <span class="dot" class:bad={a.what === "oom" || a.what === "memory"} class:warn={a.what === "unhealthy"}></span>
          <span class="name">{a.what === "memory" ? "Containers waited on memory" : `${a.name} ${a.what === "oom" ? "ran out of memory" : "is unhealthy"}`}</span>
          <span class="muted small">{ago(a.ago)}</span>
        </button>
      {/each}
    </div>
  {/if}

  <ul class="quick-list">
    {#if groups.length}
      <li class="quick-section">Projects</li>
      {#each groups as g (g.name)}
        <li class:off={g.running === 0}>
          <span class="dot" class:on={g.running > 0} class:partial={g.running > 0 && g.running < g.total} class:warn={g.sick}></span>
          <button class="name link" title={g.problem ?? (g.down ? `${g.name} is down` : `${g.running} of ${g.total} running`)} onclick={() => open("/")}>{g.name}</button>
          <span class="muted small">{g.down ? "down" : `${g.running}/${g.total}`}</span>
          {#if g.down}
            <button class="icon-btn" title="Up (docker compose up -d)" disabled={busy[g.name] || !!g.problem} onclick={() => projectDo(g, "up")}><Icon name="play" size={13} /></button>
          {:else}
            {#if g.running < g.total}
              <button class="icon-btn" title="Start all" disabled={busy[g.name]} onclick={() => projectDo(g, "start")}><Icon name="play" size={13} /></button>
            {/if}
            {#if g.running > 0}
              <button class="icon-btn" title="Stop all" disabled={busy[g.name]} onclick={() => projectDo(g, "stop")}><Icon name="stop" size={13} /></button>
            {/if}
          {/if}
        </li>
      {/each}
    {/if}

    {#if singles.length}
      {#if groups.length || machines.length}<li class="quick-section">Containers</li>{/if}
      {#each singles as c (c.id)}
        {@const on = c.state === "running"}
        <li class:off={!on}>
          <span class="dot" class:on class:warn={unhealthy(c)}></span>
          <button class="name link" title={c.image} onclick={() => open(`/containers/${c.id}`)}>{c.name}</button>
          {#if on && domainUrl(c)}
            <button class="port" title={domainUrl(c)} aria-label={`Open ${domainUrl(c)}`} onclick={() => shell.openExternal(domainUrl(c))}>↗</button>
          {/if}
          {#if on}
            {#each c.ports.filter((p) => p.public).slice(0, 2) as p (`${p.public}/${p.proto}`)}
              <button class="port" onclick={() => shell.openExternal(`http://localhost:${p.public}`)}>{p.public}</button>
            {/each}
            <button class="icon-btn" title="Stop" onclick={() => attempt(() => api.container_action(c.id, "stop"))}><Icon name="stop" size={13} /></button>
          {:else}
            <button class="icon-btn" title="Start" onclick={() => attempt(() => api.container_action(c.id, "start"))}><Icon name="play" size={13} /></button>
          {/if}
        </li>
      {/each}
    {/if}

    {#if machines.length}
      <li class="quick-section">Linux machines</li>
      {#each machines as m (m.name)}
        {@const on = m.state === "running"}
        <li class:off={!on}>
          <span class="dot" class:on></span>
          <button class="name link" title={m.distro} onclick={() => open(`/machines/${m.name}`)}>{m.name}</button>
          <span class="muted small">{m.distro}</span>
          {#if on}
            <button class="icon-btn" title="Terminal" onclick={() => open(`/machines/${m.name}`)}><Icon name="terminal" size={13} /></button>
          {:else}
            <button class="icon-btn" title="Start" disabled={busy[`m:${m.name}`]} onclick={() => machineStart(m)}><Icon name="play" size={13} /></button>
          {/if}
        </li>
      {/each}
    {/if}

    {#if !groups.length && !singles.length && !machines.length}
      <li class="muted">No containers.</li>
    {/if}
  </ul>

  <footer class="quick-foot">
    <button class="ghost-btn" onclick={() => open("/")}><Icon name="window" size={13} /> Open eDock</button>
    <button class="ghost-btn" onclick={() => open("#new-service")}><Icon name="plus" size={13} /> New service</button>
    <span class="spacer"></span>
    <button class="ghost-btn" onclick={() => quit()}>Quit</button>
  </footer>
</div>
