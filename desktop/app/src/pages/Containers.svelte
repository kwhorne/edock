<script>
  import { channel, contextMenu, dialog, navigate, shell, clipboard, confirm, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Empty, Input, Modal, Skeleton, Switch } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Logs from "../lib/Logs.svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, domainUrl, errorText, runImage, dialogs } from "../lib/state.svelte.js";
  import { ago, shortId, stateTone } from "../lib/format.js";

  let containers = $state(null);
  let error = $state(null);
  let search = $state("");
  // A path in the project's folder as ./path; a folder beside it, such as
  // app-data next to app, stays as it is.
  const rel = (s, dir) => (dir && (s === dir || s.startsWith(dir + "/") || s.startsWith(dir + ":")) ? "." + s.slice(dir.length) : s);
  // eDock's own (Linux machines, the Kubernetes node, BuildKit) have their
  // own pages; here only when asked for.
  let showSystem = $state(false);
  const systemCount = $derived(containers?.filter((c) => c.system).length ?? 0);
  // The user's own: what the header counts and the empty page speaks of.
  const mine = $derived(containers?.filter((c) => !c.system) ?? []);
  // Compose projects eDock remembers on this engine: those that are down
  // show too, with Up.
  let projects = $state([]);
  let collapsed = $state({});
  let busy = $state({});
  // On demand (eDock's engine): each Compose project's and lone
  // container's. Parked ones say so in their status.
  let onDemand = $state([]);
  const od = (kind, name) => onDemand.find((u) => u.kind === kind && u.name === name);
  const parked = (c) => !!c.status?.endsWith("(parked)");

  async function load() {
    api.compose_projects().then((p) => (projects = p), () => {});
    api.on_demand_list().then((u) => (onDemand = u), () => {});
    try {
      containers = await api.containers_list(false);
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    changes.container;
    load();
  });

  const emptyText = $derived(
    "Run one with `docker run`, or `docker compose up` in a project, or start a database or other service here. It shows up at once." +
      (systemCount ? ` (eDock's own ${systemCount === 1 ? "one is" : `${systemCount} are`} hidden: “eDock's own” above shows ${systemCount === 1 ? "it" : "them"}.)` : ""),
  );

  // Remembered projects with no containers: `docker compose down` ran.
  const down = $derived(containers ? projects.filter((p) => !containers.some((c) => c.project === p.name)) : []);

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
    const out = [...byProject.entries()].map(([project, items]) => ({
      project,
      items: items.sort((a, b) => (a.service ?? a.name).localeCompare(b.service ?? b.name)),
      running: items.filter((c) => c.state === "running").length,
      parked: items.some(parked) && items.every((c) => c.state !== "running"),
      dir: items[0].working_dir,
    }));
    for (const p of down) {
      if (!q || p.name.toLowerCase().includes(q)) out.push({ project: p.name, items: [], running: 0, dir: p.dir, down: true, problem: p.problem });
    }
    out.sort((a, b) => a.project.localeCompare(b.project));
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

  // ── A Compose project as a whole ──
  let projectLogs = $state(null);
  let projectLogsOpen = $state(false);
  function showLogs(g) {
    projectLogs = g.project;
    projectLogsOpen = true;
  }
  let editor = $state(null);
  api.editor_name().then((e) => (editor = e));
  function folderMenu(e, g) {
    contextMenu(e, [
      { label: "Open in Finder", action: () => attempt(() => api.folder_open(g.dir, "finder")) },
      { label: "Open in Terminal", action: () => attempt(() => api.folder_open(g.dir, "terminal")) },
      editor ? { label: `Open in ${editor}`, action: () => attempt(() => api.folder_open(g.dir, "editor")) } : null,
      { separator: true },
      { label: "Copy path", action: () => clipboard.writeText(g.dir).then(() => toast("Copied")) },
    ].filter(Boolean));
  }
  let up = $state(null); // { project, lines, done, error }
  let upOpen = $state(false);
  const offUp = channel("compose:progress").subscribe((p) => {
    if (!up || p.project !== up.project) return;
    if (p.done) up = { ...up, done: true, error: p.error };
    else up = { ...up, lines: [...up.lines, p.line].slice(-400) };
  });
  onDestroy(offUp);
  async function projectUp(g, build) {
    up = { project: g.project, title: `${build ? "Rebuild & up" : "Up"}: ${g.project}`, what: build ? "docker compose up -d --build" : "docker compose up -d", lines: [], done: false, error: null };
    upOpen = true;
    busy[g.project] = "up";
    try {
      await api.project_up(g.project, build);
      toast(build ? `${g.project} rebuilt and up` : `${g.project} is up`, { variant: "success" });
    } catch (e) {
      up = { ...up, done: true, error: errorText(e) };
    }
    busy[g.project] = null;
  }

  // One service made again, the rest left as they are.
  async function serviceUp(project, service, build) {
    viewOpen = false;
    up = {
      project,
      title: `${build ? "Rebuild & recreate" : "Recreate"}: ${service}`,
      what: `docker compose up -d --no-deps --force-recreate${build ? " --build" : ""} ${service}`,
      lines: [],
      done: false,
      error: null,
    };
    upOpen = true;
    busy[project] = "up";
    try {
      await api.service_up(project, service, build);
      toast(`${service} made again`, { variant: "success" });
    } catch (e) {
      up = { ...up, done: true, error: errorText(e) };
    }
    busy[project] = null;
  }

  // ── What a project is made of ──
  let view = $state(null); // { project, data, error }
  let viewOpen = $state(false);
  async function showView(g) {
    view = { project: g.project, data: null, error: null };
    viewOpen = true;
    try {
      const data = await api.compose_view(g.project);
      if (view?.project === g.project) view = { ...view, data };
    } catch (e) {
      if (view?.project === g.project) view = { ...view, error: errorText(e) };
    }
  }

  async function openProject() {
    const [dir] = await dialog.open({ title: "A folder with a compose.yaml", directory: true });
    if (!dir) return;
    const name = await attempt(() => api.compose_project_open(dir.path));
    if (name == null) return;
    await load();
    const known = containers?.some((c) => c.project === name);
    toast(known ? `${name} is here already` : `${name} added: Up starts it`, { variant: "success" });
  }

  async function forget(g) {
    if (!(await confirm(`Forget ${g.project}? Its folder, files and volumes stay; Open Compose project… adds it again.`, { confirmLabel: "Forget" }))) return;
    await attempt(() => api.compose_project_forget(g.project));
    await load();
  }

  async function remove(c) {
    if (!(await confirm(`Remove ${c.name}?`, { danger: true, confirmLabel: "Remove" }))) return;
    await act(c, "remove");
  }

  async function removeProject(g) {
    if (!(await confirm(`Remove the ${g.items.length} containers of ${g.project}? Volumes are kept, and so is the project: Up brings it back.`, { danger: true, confirmLabel: "Remove" }))) return;
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
      c.project && c.service && c.working_dir ? { separator: true } : null,
      c.project && c.service && c.working_dir ? { label: `Recreate ${c.service}`, action: () => serviceUp(c.project, c.service, false) } : null,
      c.project && c.service && c.working_dir ? { label: `Rebuild & recreate ${c.service}`, action: () => serviceUp(c.project, c.service, true) } : null,
      { separator: true },
      running ? { label: "Stop", action: () => act(c, "stop") } : { label: "Start", action: () => act(c, "start") },
      { label: "Restart", action: () => act(c, "restart") },
      running && c.state !== "paused" ? { label: "Pause", action: () => act(c, "pause") } : null,
      c.state === "paused" ? { label: "Resume", action: () => act(c, "unpause") } : null,
      { separator: true },
      ...(!c.project && !c.system && od("container", c.name) ? [{ separator: true }, ...onDemandItems("container", c.name, [c])] : []),
      { separator: true },
      { label: "Copy ID", action: () => clipboard.writeText(c.id).then(() => toast("Copied")) },
      { label: "Copy name", action: () => clipboard.writeText(c.name).then(() => toast("Copied")) },
      { separator: true },
      { label: "Remove…", action: () => remove(c) },
    ].filter(Boolean));
  }

  // On demand: parked (stopped) while nothing uses it, started again on
  // the next connection.
  const OD_TIMES = [["5m", "5 minutes"], ["15m", "15 minutes"], ["1h", "an hour"]];
  function onDemandItems(kind, name, items) {
    const u = od(kind, name);
    if (!u) return [];
    const mark = (on) => (on ? "✓ " : "");
    return [
      ...OD_TIMES.map(([v, l]) => ({ label: `${mark(u.on && u.after === v)}Park after ${l} unused`, disabled: !u.wakeable || u.auto_remove, action: () => setOnDemand(u, v, items) })),
      u.on && !OD_TIMES.some(([v]) => v === u.after) ? { label: `✓ Park after ${u.after} unused`, disabled: true } : null,
      { label: `${mark(!u.on)}Keep running`, action: () => setOnDemand(u, "off", items) },
      u.setting ? { label: `As its label says (${u.label ? `after ${u.label}` : "keep running"})`, action: () => setOnDemand(u, null, items) } : null,
      u.auto_remove ? { label: "Removed when it stops (--rm): it can't be parked", disabled: true } : u.wakeable ? null : { label: "Nothing can wake it: publish a port", disabled: true },
      u.on && u.state !== "parked" ? { separator: true } : null,
      u.on && u.state !== "parked" ? { label: "Park now", action: () => parkNow(u) } : null,
    ].filter(Boolean);
  }
  async function setOnDemand(u, value, items) {
    // Turning it on: what stops with it.
    if (!u.on && value && value !== "off") {
      const quiet = items.filter((c) => !c.ports.some((p) => p.public)).map((c) => c.service ?? c.name);
      const what = u.kind === "project" ? `${u.name} and all its services stop` : `${u.name} stops`;
      const ok = await confirm(
        `${what} when nothing has used them for ${OD_TIMES.find(([v]) => v === value)?.[1] ?? value}, and start again on the next connection to their ports or their .edock.localhost names, which waits a moment for them.` +
          (quiet.length && u.kind === "project" ? ` ${quiet.join(", ")} ${quiet.length === 1 ? "has" : "have"} no published port: while ${u.name} is parked, ${quiet.length === 1 ? "it doesn't" : "they don't"} run (a queue worker or a scheduled job waits too).` : "") +
          " What a container keeps only in memory is lost when it's parked, as with docker stop.",
        { confirmLabel: "Turn on" },
      );
      if (!ok) return;
    }
    const r = await attempt(() => api.project_on_demand(u.name, value));
    if (r) toast(r.on ? `${r.name} is on demand: parked after ${r.after} unused` : `${r.name} keeps running`, { variant: "success" });
    load();
  }
  async function parkNow(u) {
    if ((await attempt(() => api.on_demand_park(u.name))) != null) toast(`${u.name} is parked: the next connection wakes it`, { variant: "success" });
    load();
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
    <Button size="sm" variant="ghost" onclick={() => runImage()} title="A container from any image, as docker run does"><Icon name="play" size={14} /> Run image…</Button>
    <Button size="sm" variant="ghost" onclick={openProject} title="Add a folder with a compose.yaml, to start it from here"><Icon name="folder" size={14} /> Open Compose project…</Button>
    <Button size="sm" variant="primary" onclick={() => (dialogs.newService = true)}><Icon name="plus" size={14} /> New service</Button>
  </header>
  <Modal bind:open={projectLogsOpen} title={projectLogs ? `${projectLogs}: all logs` : "Logs"} description="Every service's log, as one, in time order. Click a service to hide it." size="lg">
    {#if projectLogsOpen && projectLogs}
      {#key projectLogs}
        <div class="project-logs"><Logs start={(tail, ts) => api.project_logs_follow(projectLogs, tail, ts)} /></div>
      {/key}
    {/if}
  </Modal>
  <Modal bind:open={viewOpen} title={view ? `${view.project}: what it's made of` : ""} description="As docker compose reads its files. Variables show by name only: their values stay in your files." size="lg">
    {#if view?.error}
      <Callout variant="warning" title="docker compose couldn't read it">{view.error}</Callout>
    {:else if !view?.data}
      <Skeleton lines={5} />
    {:else}
      <p class="muted small mono">{view.data.files.join(" · ")}</p>
      <div class="compose-view">
        {#each view.data.services as sv (sv.name)}
          <section class="compose-service">
            <div class="compose-service-head">
              <strong>{sv.name}</strong>
              <span class="muted mono">{sv.build ? `build ${rel(sv.build, view.data.dir)}` : sv.image}</span>
              {#each sv.profiles as p (p)}<Badge>{p}</Badge>{/each}
              <span class="spacer"></span>
              <Button size="sm" disabled={!!busy[view.project]} onclick={() => serviceUp(view.project, sv.name, false)} title="docker compose up -d --no-deps --force-recreate {sv.name}">Recreate</Button>
              {#if sv.build}<Button size="sm" disabled={!!busy[view.project]} onclick={() => serviceUp(view.project, sv.name, true)} title="… --build: its image built again first">Rebuild &amp; recreate</Button>{/if}
            </div>
            <dl class="compose-facts">
              {#if sv.ports.length}<dt>Ports</dt><dd class="mono">{sv.ports.join(", ")}</dd>{/if}
              {#if sv.volumes.length}<dt>Volumes</dt><dd class="mono">{sv.volumes.map((v) => rel(v, view.data.dir)).join(", ")}</dd>{/if}
              {#if sv.depends_on.length}<dt>Needs</dt><dd>{sv.depends_on.join(", ")}</dd>{/if}
              {#if sv.env.length}<dt>Variables</dt><dd class="mono">{sv.env.join(", ")}</dd>{/if}
            </dl>
          </section>
        {/each}
        {#each view.data.env_files as f (f.path)}
          <section class="compose-service">
            <div class="compose-service-head"><strong class="mono">{rel(f.path, view.data.dir).replace(/^\.\//, "")}</strong><span class="muted">{f.keys.length} variable{f.keys.length === 1 ? "" : "s"}</span></div>
            {#if f.keys.length}<p class="mono small">{f.keys.join(", ")}</p>{/if}
          </section>
        {/each}
      </div>
    {/if}
  </Modal>
  <Modal bind:open={upOpen} title={up?.title ?? ""} description={up ? `${up.what}, from the project's own files and folder.` : ""} size="lg">
    {#if up}
      {#if up.error}<Callout variant="danger" title="It didn't go up">{up.error}</Callout>{:else if up.done}<Callout variant="success">Up.</Callout>{/if}
      <pre class="progress compose-out">{up.lines.join("\n") || "Starting…"}</pre>
    {/if}
  </Modal>

  {#if error}
    <Callout variant="warning" title="The engine didn't answer">
      {error}
      {#if engine.status?.context === "edock"}<br />Start it from <a href="#/engine">Engine</a>, or the menu bar.{/if}
    </Callout>
  {:else if containers === null}
    <Skeleton lines={6} />
  {:else if mine.length === 0 && !showSystem && !down.length}
    <Empty
      title="No containers yet"
      description={emptyText}
    >
      {#snippet action()}<Button onclick={() => (dialogs.newService = true)}><Icon name="plus" size={14} /> New service</Button>{/snippet}
    </Empty>
  {:else}
    {#each groups as g (g.project ?? "\u0000")}
      <section class="group">
        {#if g.project}
          <div class="group-head">
            {#if g.down}
              <span class="twisty-space"></span>
            {:else}
              <button class="twisty" class:open={!collapsed[g.project]} onclick={() => (collapsed[g.project] = !collapsed[g.project])} aria-label="Show or hide {g.project}">
                <Icon name="chevron" size={14} />
              </button>
            {/if}
            <span class="dot" class:on={g.running > 0} class:partial={g.running > 0 && g.running < g.items.length}></span>
            <strong>{g.project}</strong>
            {#if od("project", g.project)?.on}
              <span class="od-badge" title="On demand: parked after {od('project', g.project).after} unused, started again on the next connection"><Icon name="moon" size={12} />{od("project", g.project).after}</span>
            {/if}
            {#if g.down}
              <span class="muted" title="No containers: docker compose down, or they were removed">down</span>
              {#if g.problem}<span class="problem" title={g.dir}>{g.problem}</span>{/if}
            {:else if g.parked}
              <span class="muted" title="Stopped while unused: the next connection to its ports or names starts it again">parked</span>
            {:else}
              <span class="muted">{g.running}/{g.items.length} running</span>
            {/if}
            {#if g.dir && !g.problem}<button class="muted path link-path" title="Open {g.dir}" onclick={(e) => folderMenu(e, g)}>{g.dir.replace(/^\/(?:Users|home)\/[^/]+/, "~")}</button>{/if}
            <span class="spacer"></span>
            {#if g.down}
            <div class="row-actions">
              <button class="icon-btn" title="Up (docker compose up -d)" disabled={!!busy[g.project] || !!g.problem} onclick={() => projectUp(g, false)}><Icon name="play" size={14} /></button>
              <button class="icon-btn" title="Rebuild & up (docker compose up -d --build)" disabled={!!busy[g.project] || !!g.problem} onclick={() => projectUp(g, true)}><Icon name="hammer" size={14} /></button>
              <button class="icon-btn" title="Forget it (its files and volumes stay)" disabled={!!busy[g.project]} onclick={() => forget(g)}><Icon name="close" size={14} /></button>
            </div>
            {:else}
            <div class="row-actions">
              <button class="icon-btn" title="All its logs" onclick={() => showLogs(g)}><Icon name="logs" size={14} /></button>
              {#if g.dir}
                <button class="icon-btn" title="What it's made of: services, ports, volumes, variables; recreate one service" onclick={() => showView(g)}><Icon name="file" size={14} /></button>
              {/if}
              {#if g.dir}
                <button class="icon-btn" title="Rebuild & up (docker compose up -d --build)" disabled={!!busy[g.project]} onclick={() => projectUp(g, true)}><Icon name="hammer" size={14} /></button>
              {/if}
              {#if od("project", g.project)}
                <button class="icon-btn" class:on={od("project", g.project).on} title="On demand: stop it while unused, start it on the next connection" onclick={(e) => contextMenu(e, onDemandItems("project", g.project, g.items))}><Icon name="moon" size={14} /></button>
              {/if}
              {#if g.running < g.items.length}
                <button class="icon-btn" title={g.parked ? "Wake (start all)" : "Start all"} disabled={!!busy[g.project]} onclick={() => projectAct(g, "start")}><Icon name="play" size={14} /></button>
              {/if}
              {#if g.running > 0}
                <button class="icon-btn" title="Stop all" disabled={!!busy[g.project]} onclick={() => projectAct(g, "stop")}><Icon name="stop" size={14} /></button>
              {/if}
              <button class="icon-btn" title="Restart all" disabled={!!busy[g.project]} onclick={() => projectAct(g, "restart")}><Icon name="restart" size={14} /></button>
              <button class="icon-btn danger" title="Remove all" disabled={!!busy[g.project]} onclick={() => removeProject(g)}><Icon name="trash" size={14} /></button>
            </div>
            {/if}
          </div>
        {:else if groups.length > 1}
          <div class="group-head plain"><strong>Containers</strong></div>
        {/if}

        {#if g.items.length && (!g.project || !collapsed[g.project])}
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
                    <div class="name">{g.project && c.service ? c.service : c.name} {#if c.system}<Badge>{c.system === "machine" ? "Linux machine" : c.system === "kubernetes" ? "Kubernetes" : c.system === "helper" ? "eDock helper" : "BuildKit"}</Badge>{/if}</div>
                    <div class="sub muted" title={c.image}>{c.image.startsWith("sha256:") ? c.image.slice(7, 19) : c.image} · {shortId(c.id)}</div>
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
                    {#if parked(c)}
                      <span title="Stopped while unused: the next connection starts it again"><Badge>parked</Badge></span>
                    {:else}
                      <Badge variant={stateTone(c.state)}>{c.state}</Badge>
                    {/if}
                    {#if c.health}<Badge variant={c.health === "healthy" ? "success" : c.health === "unhealthy" ? "danger" : "warning"}>{c.health}</Badge>{/if}
                    <div class="sub muted">{running || c.status ? c.status.replace(/ \(parked\)$/, "") : ago(c.created)}</div>
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
