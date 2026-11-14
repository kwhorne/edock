<script>
  import Router from "./Router.svelte";
  import { routes } from "./routes.js";
  import { appWindow, channel, navigate, registerCommands, route } from "@elyra/runtime";
  import { NavItem, Select, Sidebar, Toolbar } from "@elyra/ui";
  import Icon from "./lib/Icon.svelte";
  import { checkForUpdates } from "./lib/updates.js";
  import { engine, startWatching, useContext, attempt } from "./lib/state.svelte.js";
  import { getTheme, applyTheme } from "./theme.js";
  import { api } from "./bindings";

  let theme = $state(getTheme());
  const quick = $derived($route.path === "/quick");

  startWatching();

  // The main window starts hidden (eDock lives in the menu bar); show it
  // on the first launch of a session.
  $effect(() => {
    if (!quick) appWindow.show();
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
    channel("navigate").subscribe((path) => {
      if (path) navigate(path);
    });
  }

  registerCommands([
    { id: "go.containers", title: "Containers", action: () => navigate("/") },
    { id: "go.images", title: "Images", action: () => navigate("/images") },
    { id: "go.volumes", title: "Volumes", action: () => navigate("/volumes") },
    { id: "go.networks", title: "Networks", action: () => navigate("/networks") },
    { id: "go.kubernetes", title: "Kubernetes", action: () => navigate("/kubernetes") },
    { id: "go.engine", title: "Engine & contexts", action: () => navigate("/engine") },
    { id: "app.update", title: "Check for updates", action: checkForUpdates },
    { id: "engine.start", title: "Start eDock engine", action: () => attempt(() => api.daemon_start(), "eDock engine started") },
  ]);

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
    <div class="body">
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
              {status.cpus} CPU · {(status.memory / 1e9).toFixed(1)} GB<br />
              {status.os}
            </div>
          {/if}
        {/snippet}
      </Sidebar>
      <main class="content">
        <Router {routes} />
      </main>
    </div>
  </div>
{/if}
