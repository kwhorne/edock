<!-- The build history: each `docker build` on eDock. -->
<script>
  import { navigate } from "@elyra/runtime";
  import { Badge, Button, Callout, Empty, Skeleton, Table } from "@elyra/ui";
  import { api } from "../bindings";
  import { engine, useContext, errorText } from "../lib/state.svelte.js";
  import { ago } from "../lib/format.js";

  let builds = $state(null);
  let error = $state(null);
  let query = $state({});
  const onEdock = $derived(engine.status?.context === "edock");

  async function load() {
    if (!onEdock) return;
    try {
      builds = (await api.builds_list()).map((b) => ({ ...b, key: `${b.builder}/${b.id}` }));
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }
  $effect(() => {
    onEdock;
    load();
  });

  const TONE = { Completed: "success", Running: "accent", Error: "danger", Canceled: "warning" };
  // How long it ran. A build still running, or cut off, has Go's zero time
  // (0001-01-01) for its end: no length.
  function took(b) {
    if (!b.started || !b.completed || b.completed.startsWith("0001-")) return "—";
    const s = (Date.parse(b.completed) - Date.parse(b.started)) / 1000;
    if (!(s >= 0)) return "—";
    if (s < 60) return `${s.toFixed(1)} s`;
    const whole = Math.round(s);
    return `${Math.floor(whole / 60)} min ${whole % 60} s`;
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Builds</h1>
    <span class="muted">{builds ? builds.length : ""}</span>
    <span class="spacer"></span>
    {#if onEdock}<Button size="sm" variant="ghost" onclick={load}>Refresh</Button>{/if}
  </header>

  {#if !onEdock}
    <Callout variant="info" title="The build history is eDock's engine's">
      This window shows <strong>{engine.status?.context}</strong>.
      <div class="callout-action"><Button size="sm" onclick={() => useContext("edock")}>Switch to eDock</Button></div>
    </Callout>
  {:else if error}
    <Callout variant="warning" title="Couldn't read the build history">{error}</Callout>
  {:else if builds === null}
    <Skeleton lines={5} />
  {:else if builds.length === 0}
    <Empty title="No builds yet" description="docker build on eDock (after Engine › Command line › Set up) shows here: each build's steps, what was cached, its logs and errors." />
  {:else}
    <Table
      rows={builds}
      key="key"
      bind:query
      search
      sortable={["name", "status", "started"]}
      label="Builds"
      onrowclick={(b) => navigate(`/builds/${encodeURIComponent(b.builder)}/${b.id}`)}
      columns={[
        { key: "name", label: "Build" },
        { key: "status", label: "Status", width: 120 },
        { key: "steps", label: "Steps", width: 140 },
        { key: "took", label: "Took", width: 110 },
        { key: "started", label: "Started", width: 130 },
      ]}
    >
      {#snippet cell(row, column)}
        {#if column.key === "status"}
          <Badge variant={TONE[row.status] ?? "neutral"}>{row.status}</Badge>
        {:else if column.key === "steps"}
          {row.steps} <span class="muted">({row.cached} cached)</span>
        {:else if column.key === "took"}
          {took(row)}
        {:else if column.key === "started"}
          {row.started ? ago(Date.parse(row.started) / 1000) : "—"}
        {:else}
          <span class="mono">{row.name}</span>
        {/if}
      {/snippet}
    </Table>
  {/if}
</div>
