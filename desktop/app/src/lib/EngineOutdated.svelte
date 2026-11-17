<!-- Above every page: the engine still runs the version before an update
     (it keeps running until it starts again), with Restart engine. -->
<script>
  import { confirm } from "@elyra/runtime";
  import { Button } from "@elyra/ui";
  import { api } from "../bindings";
  import { engine, attempt, refreshEngine } from "./state.svelte.js";

  const s = $derived(engine.status);
  // A checkout's engine or app says 0.0.0: no comparing then.
  const outdated = $derived(
    s?.context === "edock" && s?.connected && s.version && s.app_version && s.version !== s.app_version && s.version !== "0.0.0" && s.app_version !== "0.0.0",
  );
  // Later: not again until the app starts again, or the versions change.
  let later = $state(null);
  let busy = $state(false);

  async function restart() {
    if (!(await confirm(`Restart the engine to run ${s.app_version}? Running containers stop and start again.`, { confirmLabel: "Restart engine" }))) return;
    busy = true;
    await attempt(() => api.daemon_restart(), `The engine runs ${s.app_version} now`);
    busy = false;
    await refreshEngine();
  }
</script>

{#if outdated && later !== `${s.version}→${s.app_version}`}
  <div class="engine-outdated" role="status">
    <span><strong>eDock was updated to {s.app_version}</strong>, and the engine still runs {s.version}.</span>
    <span class="spacer"></span>
    <Button size="sm" variant="primary" loading={busy} onclick={restart}>Restart engine</Button>
    <Button size="sm" variant="ghost" onclick={() => (later = `${s.version}→${s.app_version}`)}>Later</Button>
  </div>
{/if}
