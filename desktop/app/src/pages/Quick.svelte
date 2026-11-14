<!-- The menu bar item's popover: the engine, and what runs, a click away. -->
<script>
  import { quit, shell } from "@elyra/runtime";
  import { Button } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, domainUrl } from "../lib/state.svelte.js";

  let containers = $state([]);

  async function load() {
    try {
      containers = await api.containers_list(false);
    } catch {
      containers = [];
    }
  }

  $effect(() => {
    changes.container;
    load();
  });

  const mine = $derived(containers.filter((c) => !c.system));
  const running = $derived(mine.filter((c) => c.state === "running"));
  const stopped = $derived(mine.filter((c) => c.state !== "running").slice(0, 5));
  const s = $derived(engine.status);

  const open = (path) => api.window_open(path ?? null);
</script>

<div class="quick">
  <header class="quick-head">
    <span class="dot" class:on={s?.connected && s?.state !== "stopped"}></span>
    <strong>eDock</strong>
    <span class="muted">{s?.state === "stopped" ? `${s.context} stopped` : s?.connected ? `${s.context} · ${running.length} running` : `${s?.context ?? ""} not running`}</span>
  </header>

  {#if s?.context === "edock" && (!s?.connected || s?.state === "stopped")}
    <div class="quick-empty">
      <Button size="sm" variant="primary" onclick={() => attempt(() => api.daemon_start(), "eDock engine started")}>Start engine</Button>
    </div>
  {/if}

  <ul class="quick-list">
    {#each running as c (c.id)}
      <li>
        <span class="dot on"></span>
        <button class="name link" title={c.image} onclick={() => open(`/containers/${c.id}`)}>{c.project && c.service ? `${c.project}/${c.service}` : c.name}</button>
        {#if domainUrl(c)}
          <button class="port" title={domainUrl(c)} onclick={() => shell.openExternal(domainUrl(c))}>↗</button>
        {/if}
        {#each c.ports.filter((p) => p.public).slice(0, 2) as p (`${p.public}/${p.proto}`)}
          <button class="port" onclick={() => shell.openExternal(`http://localhost:${p.public}`)}>{p.public}</button>
        {/each}
        <button class="icon-btn" title="Stop" onclick={() => attempt(() => api.container_action(c.id, "stop"))}><Icon name="stop" size={13} /></button>
      </li>
    {/each}
    {#each stopped as c (c.id)}
      <li class="off">
        <span class="dot"></span>
        <button class="name link" onclick={() => open(`/containers/${c.id}`)}>{c.project && c.service ? `${c.project}/${c.service}` : c.name}</button>
        <button class="icon-btn" title="Start" onclick={() => attempt(() => api.container_action(c.id, "start"))}><Icon name="play" size={13} /></button>
      </li>
    {/each}
    {#if containers.length === 0}
      <li class="muted">No containers.</li>
    {/if}
  </ul>

  <footer class="quick-foot">
    <button class="ghost-btn" onclick={() => open("/")}><Icon name="window" size={13} /> Open eDock</button>
    <span class="spacer"></span>
    <button class="ghost-btn" onclick={() => quit()}>Quit</button>
  </footer>
</div>
