<script>
  import { confirm, prompt, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton, Table } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, attempt } from "../lib/state.svelte.js";

  let networks = $state(null);
  let error = $state(null);
  let query = $state({});

  async function load() {
    try {
      networks = await api.networks_list();
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  $effect(() => {
    changes.network;
    load();
  });

  async function create() {
    const name = await prompt("New bridge network", { placeholder: "backend", confirmLabel: "Create" });
    if (name?.trim()) await attempt(() => api.network_create(name.trim()), `Created ${name.trim()}`);
  }

  async function remove(n) {
    if (!(await confirm(`Remove the network ${n.name}?`, { danger: true, confirmLabel: "Remove" }))) return;
    await attempt(() => api.network_remove(n.id));
  }

  async function prune() {
    if (!(await confirm("Remove every network no container uses?", { danger: true, confirmLabel: "Remove" }))) return;
    const n = await attempt(() => api.networks_prune());
    if (n != null) toast(`Removed ${n} network${n === 1 ? "" : "s"}`, { variant: "success" });
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Networks</h1>
    <span class="muted">{networks ? networks.length : ""}</span>
    <span class="spacer"></span>
    <Button size="sm" variant="ghost" onclick={prune}><Icon name="broom" size={14} /> Prune unused</Button>
    <Button size="sm" variant="primary" onclick={create}><Icon name="plus" size={14} /> New network</Button>
  </header>

  {#if error}
    <Callout variant="warning" title="The engine didn't answer">{error}</Callout>
  {:else if networks === null}
    <Skeleton lines={6} />
  {:else}
    <Table
      rows={networks}
      bind:query
      search
      sortable={["name", "driver", "project"]}
      label="Networks"
      resizable
      persist="networks"
      columns={[
        { key: "name", label: "Name" },
        { key: "driver", label: "Driver", width: 90 },
        { key: "subnet", label: "Subnet", width: 150 },
        { key: "project", label: "Project", width: 130 },
        { key: "containers", label: "Containers", width: 240 },
      ]}
    >
      {#snippet cell(row, column)}
        {#if column.key === "containers"}
          {#each row.containers as c (c)}<Badge variant="accent">{c}</Badge> {/each}
        {:else if column.key === "name"}
          {row.name} {#if row.builtin}<Badge>built-in</Badge>{/if} {#if row.internal}<Badge variant="warning">internal</Badge>{/if}
        {:else if column.key === "subnet"}
          <span class="mono">{row.subnet}</span>
        {:else}
          {row[column.key] ?? ""}
        {/if}
      {/snippet}
      {#snippet actions(row)}
        {#if !row.builtin}
          <button class="icon-btn danger" title="Remove" onclick={() => remove(row)}><Icon name="trash" size={14} /></button>
        {/if}
      {/snippet}
    </Table>
  {/if}
</div>
