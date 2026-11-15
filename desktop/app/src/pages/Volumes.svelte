<script>
  import { confirm, dialog, navigate, prompt, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton, Table } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, attempt } from "../lib/state.svelte.js";
  import { bytes } from "../lib/format.js";

  let volumes = $state(null);
  let error = $state(null);
  let query = $state({});

  async function load() {
    try {
      volumes = await api.volumes_list();
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  $effect(() => {
    changes.volume;
    load();
  });

  async function create() {
    const name = await prompt("New volume", { placeholder: "data", confirmLabel: "Create" });
    if (name?.trim()) await attempt(() => api.volume_create(name.trim()), `Created ${name.trim()}`);
  }

  async function snapshot(v) {
    const note = await prompt(`Snapshot ${v.name}'s files. A note, if you like:`, { placeholder: "before the migration", confirmLabel: "Take snapshot" });
    if (note == null) return;
    await attempt(() => api.volume_snapshot(v.name, note.trim() || null), `Snapshot of ${v.name} taken`);
  }

  async function clone(v) {
    const target = await prompt(`Copy ${v.name} into a new volume:`, { defaultValue: `${v.name}-copy`, confirmLabel: "Clone" });
    if (target?.trim()) await attempt(() => api.volume_clone(v.name, target.trim()), `${target.trim()} is a copy of ${v.name}`);
  }

  async function exportIt(v) {
    const file = await dialog.save({ title: `Export ${v.name}`, defaultName: `${v.name}.tar` });
    if (file) await attempt(() => api.volume_export(v.name, file), `${v.name} saved to ${file.name}`);
  }

  async function importIt() {
    const [file] = await dialog.open({ title: "Import a volume from a .tar", filters: [{ name: "Tar archive", extensions: ["tar"] }] });
    if (!file) return;
    const name = await prompt("The new volume's name:", { defaultValue: file.name.replace(/\.tar$/, ""), confirmLabel: "Import" });
    if (name?.trim()) await attempt(() => api.volume_import(name.trim(), file), `Imported ${name.trim()}`);
  }

  async function remove(v) {
    if (!(await confirm(`Remove ${v.name} and everything in it?`, { danger: true, confirmLabel: "Remove" }))) return;
    await attempt(() => api.volume_remove(v.name));
  }

  async function prune() {
    if (!(await confirm("Remove every volume no container uses, and the data in them?", { danger: true, confirmLabel: "Remove" }))) return;
    const freed = await attempt(() => api.volumes_prune());
    if (freed != null) toast(`Freed ${bytes(freed)}`, { variant: "success" });
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Volumes</h1>
    <span class="muted">{volumes ? volumes.length : ""}</span>
    <span class="spacer"></span>
    <Button size="sm" variant="ghost" onclick={prune}><Icon name="broom" size={14} /> Prune unused</Button>
    <Button size="sm" variant="ghost" onclick={importIt}><Icon name="upload" size={14} /> Import…</Button>
    <Button size="sm" variant="primary" onclick={create}><Icon name="plus" size={14} /> New volume</Button>
  </header>

  {#if error}
    <Callout variant="warning" title="The engine didn't answer">{error}</Callout>
  {:else if volumes === null}
    <Skeleton lines={6} />
  {:else}
    <Table
      rows={volumes}
      bind:query
      search
      sortable={["name", "project", "size"]}
      label="Volumes"
      onrowclick={(row) => navigate(`/volumes/${encodeURIComponent(row.name)}`)}
      resizable
      persist="volumes"
      columns={[
        { key: "name", label: "Name" },
        { key: "project", label: "Project", width: 140 },
        { key: "used_by", label: "Used by", width: 220 },
        { key: "size", label: "Size", align: "end", width: 100, format: bytes },
        { key: "driver", label: "Driver", width: 90 },
      ]}
    >
      {#snippet cell(row, column)}
        {#if column.key === "used_by"}
          {#if row.used_by.length}
            {#each row.used_by as c (c)}<Badge variant="accent">{c}</Badge> {/each}
          {:else}<span class="muted">unused</span>{/if}
        {:else if column.key === "name"}
          <span class="mono" title={row.mountpoint}>{row.name}</span>
        {:else if column.key === "project"}
          {row.project ?? ""}
        {:else}
          {column.format ? column.format(row[column.key], row) : row[column.key]}
        {/if}
      {/snippet}
      {#snippet actions(row)}
        <button class="icon-btn" title="Its files" onclick={() => navigate(`/volumes/${encodeURIComponent(row.name)}`)}><Icon name="folder" size={14} /></button>
        <button class="icon-btn" title="Snapshot its files" onclick={() => snapshot(row)}><Icon name="camera" size={14} /></button>
        <button class="icon-btn" title="Clone into a new volume" onclick={() => clone(row)}><Icon name="copy" size={14} /></button>
        <button class="icon-btn" title="Export to a .tar" onclick={() => exportIt(row)}><Icon name="download" size={14} /></button>
        <button class="icon-btn danger" title="Remove" onclick={() => remove(row)}><Icon name="trash" size={14} /></button>
      {/snippet}
      {#snippet empty()}<span class="muted">No volumes.</span>{/snippet}
    </Table>
  {/if}
</div>
