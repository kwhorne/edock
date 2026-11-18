<script>
  import Router from "./Router.svelte";
  import { routes } from "./routes.js";
  import { appWindow, channel, navigate, onAgent, registerCommands, route, toast } from "@elyra/runtime";
  import { NavItem, Select, Sidebar, Toolbar } from "@elyra/ui";
  import Icon from "./lib/Icon.svelte";
  import { checkForUpdates } from "./lib/updates.js";
  import RunImage from "./lib/RunImage.svelte";
  import NewService from "./lib/NewService.svelte";
  import EngineOutdated from "./lib/EngineOutdated.svelte";
  import { startSearch } from "./lib/search.js";
  import { startRecipe } from "./lib/recipes.js";
  import { runImage, dialogs, engine, startWatching, useContext, attempt } from "./lib/state.svelte.js";
  import { getTheme, applyTheme } from "./theme.js";
  import { api } from "./bindings";

  let theme = $state(getTheme());
  const quick = $derived($route.path === "/quick");

  startWatching();

  // The main window starts hidden (eDock lives in the menu bar); show it
  // on the first launch of a session, with the welcome the very first time.
  $effect(() => {
    if (quick) return;
    appWindow.show();
    api.welcome_needed().then((first) => first && navigate("/welcome"));
  });

  // The menu bar item's menu, and pages the popover asks for. Handlers,
  // not $effects: the same item clicked twice must act twice.
  if (!location.hash.startsWith("#/quick")) {
    channel("tray").subscribe((id) => {
      if (id === "open") appWindow.show().then(() => appWindow.focus());
      if (id === "engine:start") attempt(() => api.daemon_start(), "eDock engine started");
      if (id === "engine:stop") attempt(() => api.daemon_stop(), "eDock engine stopped");
      if (id === "update") appWindow.show().then(checkForUpdates);
    });
    // Pages, and from the popover the dialogs that live in this window.
    channel("navigate").subscribe((path) => {
      if (path === "#new-service") {
        navigate("/");
        dialogs.newService = true;
      } else if (path) navigate(path);
    });
    // An AI agent at work (MCP): said when it changes something; reading is quiet.
    const CHANGES = new Set([
      "container_action", "project_action", "project_up", "image_pull", "container_run", "container_exec", "recipe_start",
      "volume_clone", "container_snapshot", "volume_snapshot", "container_snapshot_restore", "volume_snapshot_restore",
      "image_run", "k8s_load", "compose_project_open", "compose_project_forget", "service_up", "project_on_demand", "on_demand_park",
    ]);
    onAgent((a) => {
      if (a.phase === "started" && CHANGES.has(a.tool)) toast(`${a.client} runs ${a.tool.replace(/_/g, " ")}…`);
      if (a.phase === "finished" && CHANGES.has(a.tool) && a.ok === false) toast(`${a.client}: ${a.tool.replace(/_/g, " ")} failed`, { variant: "error" });
    });
  }

  const COMMANDS = [
    { id: "go.containers", title: "Containers", action: () => navigate("/") },
    { id: "run.image", title: "Run image…", action: () => runImage() },
    { id: "go.images", title: "Images", action: () => navigate("/images") },
    { id: "go.volumes", title: "Volumes", action: () => navigate("/volumes") },
    { id: "go.networks", title: "Networks", action: () => navigate("/networks") },
    { id: "go.machines", title: "Linux machines", action: () => navigate("/machines") },
    { id: "go.builds", title: "Builds", action: () => navigate("/builds") },
    { id: "go.snapshots", title: "Snapshots", action: () => navigate("/snapshots") },
    { id: "go.requests", title: "Requests: HTTP inspector", action: () => navigate("/requests") },
    { id: "go.welcome", title: "Welcome: set eDock up", action: () => navigate("/welcome") },
    { id: "go.kubernetes", title: "Kubernetes", action: () => navigate("/kubernetes") },
    { id: "go.engine", title: "Engine & contexts", action: () => navigate("/engine") },
    { id: "app.update", title: "Check for updates", action: checkForUpdates },
  ];
  const START = { id: "engine.start", title: "Start eDock engine", action: () => attempt(() => api.daemon_start(), "eDock engine started") };
  // Start is offered while eDock's engine isn't running.
  const startable = $derived(engine.status?.context === "edock" && ["off", "stopped"].includes(engine.status?.state));
  $effect(() => registerCommands(startable ? [...COMMANDS, START] : COMMANDS));
  // ⌘K: find things by name, and start a service from a recipe (found by
  // its name or image, after the containers).
  if (!location.hash.startsWith("#/quick")) {
    startSearch(startRecipe);
  }

  function cycleTheme() {
    theme = theme === "auto" ? "light" : theme === "light" ? "dark" : "auto";
    applyTheme(theme);
  }

  const contextOptions = $derived(
    engine.contexts.map((c) => ({ value: c.name, label: c.name + (c.available ? "" : " (no socket)") })),
  );
  const status = $derived(engine.status);
</script>

{#if quick}
  <Router {routes} />
{:else}
  <div class="window">
    <Toolbar title="eDock">
      <span class="engine-pill" class:up={status?.connected && status?.state !== "stopped"} title={status?.error ?? status?.host ?? ""}>
        <span class="dot"></span>
        {#if status?.state === "stopped"}
          {status.context}: stopped
        {:else if status?.connected}
          {status.name || status.context} · {status.version}
        {:else if status}
          {status.context}: not running
        {:else}
          Connecting…
        {/if}
      </span>
      {#snippet actions()}
        {#if contextOptions.length}
          <Select
            class="context-select"
            value={status?.context}
            options={contextOptions}
            onchange={(v) => useContext(v)}
          />
        {/if}
        <button class="ghost-btn" onclick={cycleTheme} title="Theme: {theme}">
          {theme === "dark" ? "☾" : theme === "light" ? "☀" : "◐"}
        </button>
      {/snippet}
    </Toolbar>
    <div class="window-body">
      <Sidebar>
        <NavItem path="/" count={status?.running ?? 0}>
          {#snippet icon()}<Icon name="box" />{/snippet}
          Containers
        </NavItem>
        <NavItem path="/images">
          {#snippet icon()}<Icon name="layers" />{/snippet}
          Images
        </NavItem>
        <NavItem path="/volumes">
          {#snippet icon()}<Icon name="disk" />{/snippet}
          Volumes
        </NavItem>
        <NavItem path="/networks">
          {#snippet icon()}<Icon name="network" />{/snippet}
          Networks
        </NavItem>
        <NavItem path="/snapshots">
          {#snippet icon()}<Icon name="camera" />{/snippet}
          Snapshots
        </NavItem>
        <NavItem path="/requests">
          {#snippet icon()}<Icon name="activity" />{/snippet}
          Requests
        </NavItem>
        <NavItem path="/builds">
          {#snippet icon()}<Icon name="hammer" />{/snippet}
          Builds
        </NavItem>
        <NavItem path="/machines">
          {#snippet icon()}<Icon name="monitor" />{/snippet}
          Machines
        </NavItem>
        <NavItem path="/kubernetes">
          {#snippet icon()}<Icon name="helm" />{/snippet}
          Kubernetes
        </NavItem>
        {#snippet footer()}
          <NavItem path="/engine">
            {#snippet icon()}<Icon name="engine" />{/snippet}
            Engine
          </NavItem>
          {#if status?.connected}
            <div class="engine-facts">
              {status.cpus} CPU{status.memory ? ` · ${(status.memory / 1e9).toFixed(1)} GB` : ""}<br />
              {status.os}
            </div>
          {/if}
        {/snippet}
      </Sidebar>
      <main class="content">
        <EngineOutdated />
        <Router {routes} />
      </main>
      <RunImage />
      <NewService bind:open={dialogs.newService} />
    </div>
  </div>
{/if}
