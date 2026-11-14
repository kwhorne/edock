<script>
  import { contextMenu, navigate, shell, clipboard, confirm, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Empty, Input, Skeleton, Switch } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import NewService from "../lib/NewService.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, domainUrl } from "../lib/state.svelte.js";
  import { ago, shortId, stateTone } from "../lib/format.js";

  let containers = $state(null);
  let error = $state(null);
  let search = $state("");
  let newService = $state(false);
  // eDock's own (Linux machines, the Kubernetes node, BuildKit) have their
  // own pages; here only when asked for.
  let showSystem = $state(false);
  const systemCount = $derived(containers?.filter((c) => c.system).length ?? 0);
  // The user's own: what the header counts and the empty page speaks of.
  const mine = $derived(containers?.filter((c) => !c.system) ?? []);
  let collapsed = $state({});
  let busy = $state({});

  async function load() {
    try {
      containers = await api.containers_list(false);
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  $effect(() => {
    changes.container;
    load();
  });

  // Compose projects first (by name), then the containers on their own.
  const groups = $derived.by(() => {
    if (!containers) return [];
    const q = search.trim().toLowerCase();
    const list = containers.filter(
      (c) =>
        (showSystem || !c.system) &&
        (!q || c.name.toLowerCase().includes(q) || c.image.toLowerCase().includes(q) || (c.project ?? "").toLowerCase().includes(q)),
    );
    const byProject = new Map();
    const single = [];
    for (const c of list) {
      if (c.project) {
        if (!byProject.has(c.project)) byProject.set(c.project, []);
        byProject.get(c.project).push(c);
      } else single.push(c);
    }
    const out = [...byProject.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([project, items]) => ({
        project,
        items: items.sort((a, b) => (a.service ?? a.name).localeCompare(b.service ?? b.name)),
        running: items.filter((c) => c.state === "running").length,
        dir: items[0].working_dir,
      }));
    single.sort((a, b) => (a.state === "running") === (b.state === "running") ? a.name.localeCompare(b.name) : a.state === "running" ? -1 : 1);
    if (single.length) out.push({ project: null, items: single });
    return out;
  });

  async function act(c, action) {
    busy[c.id] = action;
    await attempt(() => api.container_action(c.id, action));
    busy[c.id] = null;
  }

  async function projectAct(g, action) {
    busy[g.project] = action;
    await attempt(() => api.project_action(g.project, action));
    busy[g.project] = null;
  }

  async function remove(c) {
    if (!(await confirm(`Remove ${c.name}?`, { danger: true, confirmLabel: "Remove" }))) return;
    await act(c, "remove");
  }

  async function removeProject(g) {
    if (!(await confirm(`Remove the ${g.items.length} containers of ${g.project}? Volumes are kept.`, { danger: true, confirmLabel: "Remove" }))) return;
    await projectAct(g, "remove");
  }

  function openPort(p) {
    shell.openExternal(`http://localhost:${p.public}`);
  }

  function menu(e, c) {
    const running = c.state === "running";
    contextMenu(e, [
      { label: "Open", action: () => navigate(`/containers/${c.id}`) },
      { label: "Terminal", disabled: !running, action: () => navigate(`/containers/${c.id}?tab=terminal`) },
      { label: "Logs", action: () => navigate(`/containers/${c.id}?tab=logs`) },
      { separator: true },
      running ? { label: "Stop", action: () => act(c, "stop") } : { label: "Start", action: () => act(c, "start") },
      { label: "Restart", action: () => act(c, "restart") },
      running && c.state !== "paused" ? { label: "Pause", action: () => act(c, "pause") } : null,
      c.state === "paused" ? { label: "Resume", action: () => act(c, "unpause") } : null,
      { separator: true },
      { label: "Copy ID", action: () => clipboard.writeText(c.id).then(() => toast("Copied")) },
      { label: "Copy name", action: () => clipboard.writeText(c.name).then(() => toast("Copied")) },
      { separator: true },
      { label: "Remove…", action: () => remove(c) },
    ].filter(Boolean));
  }

  async function prune() {
    if (!(await confirm("Remove every stopped container?", { danger: true, confirmLabel: "Remove" }))) return;
    const freed = await attempt(() => api.containers_prune());
    if (freed != null) toast("Stopped containers removed", { variant: "success" });
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Containers</h1>
    <span class="muted">{containers ? `${mine.filter((c) => c.state === "running").length} running · ${mine.length} in all` : ""}</span>
    <span class="spacer"></span>
    <div class="search"><Icon name="search" size={14} /><Input bind:value={search} placeholder="Filter" aria-label="Filter containers" /></div>
    {#if systemCount}
      <label class="toggle" title="Linux machines, the Kubernetes node and BuildKit"><Switch bind:checked={showSystem} /> eDock's own ({systemCount})</label>
    {/if}
    <Button size="sm" variant="ghost" onclick={prune} title="Remove stopped containers (Linux machines and Kubernetes are kept)"><Icon name="broom" size={14} /> Prune</Button>
    <Button size="sm" variant="primary" onclick={() => (newService = true)}><Icon name="plus" size={14} /> New service</Button>
  </header>
  <NewService bind:open={newService} />

  {#if error}
    <Callout variant="warning" title="The engine didn't answer">
      {error}
      {#if engine.status?.context === "edock"}<br />Start it from <a href="#/engine">Engine</a>, or the menu bar.{/if}
    </Callout>
  {:else if containers === null}
    <Skeleton lines={6} />
  {:else if mine.length === 0 && !showSystem}
    <Empty
      title="No containers yet"
      description="Run one with `docker run`, or `docker compose up` in a project, or start a database or other service here. It shows up at once.{systemCount ? ` (eDock's own ${systemCount === 1 ? "one is" : `${systemCount} are`} hidden: “eDock's own” above shows ${systemCount === 1 ? "it" : "them"}.)` : ""}"
    >
      {#snippet action()}<Button onclick={() => (newService = true)}><Icon name="plus" size={14} /> New service</Button>{/snippet}
    </Empty>
  {:else}
    {#each groups as g (g.project ?? "\u0000")}
      <section class="group">
        {#if g.project}
          <div class="group-head">
            <button class="twisty" class:open={!collapsed[g.project]} onclick={() => (collapsed[g.project] = !collapsed[g.project])} aria-label="Show or hide {g.project}">
              <Icon name="chevron" size={14} />
            </button>
            <span class="dot" class:on={g.running > 0} class:partial={g.running > 0 && g.running < g.items.length}></span>
            <strong>{g.project}</strong>
            <span class="muted">{g.running}/{g.items.length} running</span>
            {#if g.dir}<span class="muted path" title={g.dir}>{g.dir.replace(/^\/(?:Users|home)\/[^/]+/, "~")}</span>{/if}
            <span class="spacer"></span>
            <div class="row-actions">
              {#if g.running < g.items.length}
                <button class="icon-btn" title="Start all" disabled={!!busy[g.project]} onclick={() => projectAct(g, "start")}><Icon name="play" size={14} /></button>
              {/if}
              {#if g.running > 0}
                <button class="icon-btn" title="Stop all" disabled={!!busy[g.project]} onclick={() => projectAct(g, "stop")}><Icon name="stop" size={14} /></button>
              {/if}
              <button class="icon-btn" title="Restart all" disabled={!!busy[g.project]} onclick={() => projectAct(g, "restart")}><Icon name="restart" size={14} /></button>
              <button class="icon-btn danger" title="Remove all" disabled={!!busy[g.project]} onclick={() => removeProject(g)}><Icon name="trash" size={14} /></button>
            </div>
          </div>
        {:else if groups.length > 1}
          <div class="group-head plain"><strong>Containers</strong></div>
        {/if}

        {#if !g.project || !collapsed[g.project]}
          <ul class="rows" class:nested={!!g.project}>
            {#each g.items as c (c.id)}
              {@const running = c.state === "running"}
              <li>
                <div
                  class="row"
                  role="button"
                  tabindex="0"
                  onclick={() => navigate(`/containers/${c.id}`)}
                  onkeydown={(e) => e.key === "Enter" && navigate(`/containers/${c.id}`)}
                  oncontextmenu={(e) => menu(e, c)}
                >
                  <span class="dot" class:on={running} class:warn={c.state === "paused" || c.state === "restarting" || c.health === "unhealthy"}></span>
                  <div class="main">
                    <div class="name">{g.project && c.service ? c.service : c.name} {#if c.system}<Badge>{c.system === "machine" ? "Linux machine" : c.system === "kubernetes" ? "Kubernetes" : "BuildKit"}</Badge>{/if}</div>
                    <div class="sub muted">{c.image} · {shortId(c.id)}</div>
                    {#if domainUrl(c)}
                      <button class="domain" title="Open {domainUrl(c)}" onclick={(e) => { e.stopPropagation(); shell.openExternal(domainUrl(c)); }}>
                        {domainUrl(c).replace(/^https?:\/\//, "")}
                      </button>
                    {/if}
                  </div>
                  <div class="ports">
                    {#each c.ports.filter((p) => p.public) as p (`${p.public}/${p.proto}`)}
                      <button class="port" title="Open http://localhost:{p.public}" onclick={(e) => { e.stopPropagation(); openPort(p); }}>
                        {p.public}<span class="muted">→{p.private}</span>
                      </button>
                    {/each}
                  </div>
                  <div class="state">
                    <Badge variant={stateTone(c.state)}>{c.state}</Badge>
                    {#if c.health}<Badge variant={c.health === "healthy" ? "success" : c.health === "unhealthy" ? "danger" : "warning"}>{c.health}</Badge>{/if}
                    <div class="sub muted">{running ? c.status : ago(c.created)}</div>
                  </div>
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                  <div class="row-actions" onclick={(e) => e.stopPropagation()}>
                    {#if running}
                      <button class="icon-btn" title="Terminal" onclick={() => navigate(`/containers/${c.id}?tab=terminal`)}><Icon name="terminal" size={14} /></button>
                      <button class="icon-btn" title="Stop" disabled={!!busy[c.id]} onclick={() => act(c, "stop")}><Icon name="stop" size={14} /></button>
                    {:else}
                      <button class="icon-btn" title="Start" disabled={!!busy[c.id]} onclick={() => act(c, "start")}><Icon name="play" size={14} /></button>
                    {/if}
                    <button class="icon-btn" title="Restart" disabled={!!busy[c.id]} onclick={() => act(c, "restart")}><Icon name="restart" size={14} /></button>
                    <button class="icon-btn danger" title="Remove" disabled={!!busy[c.id]} onclick={() => remove(c)}><Icon name="trash" size={14} /></button>
                  </div>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
    {#if groups.length === 0}
      <Empty title="Nothing matches" description={search.trim() ? `No container's name, image or project has “${search.trim()}”.` : "Nothing to show."} />
    {/if}
  {/if}
</div>
