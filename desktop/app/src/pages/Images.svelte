<script>
  import { channel, confirm, navigate, prompt, toast } from "@elyra/runtime";
  import { onDestroy } from "svelte";
  import { Badge, Button, Callout, Skeleton, Table } from "@elyra/ui";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, errorText } from "../lib/state.svelte.js";
  import { ago, bytes, shortId } from "../lib/format.js";

  let images = $state(null);
  let error = $state(null);
  let query = $state({});
  let pulling = $state({});

  async function load() {
    try {
      images = (await api.images_list()).map((i) => ({ ...i, key: `${i.id}:${i.repository}:${i.tag}` }));
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    changes.image;
    load();
  });

  const offPull = channel("pull").subscribe((p) => {
    if (!p) return;
    if (p.done) delete pulling[p.reference];
    else pulling[p.reference] = p.status;
  });

  onDestroy(offPull);

  async function pull() {
    const ref = await prompt("Image to pull", { placeholder: "nginx:latest", confirmLabel: "Pull" });
    if (!ref?.trim()) return;
    const r = ref.trim();
    pulling[r] = "Starting…";
    const ok = await attempt(() => api.image_pull(r).then(() => true));
    delete pulling[r];
    if (ok) toast(`Pulled ${r}`, { variant: "success" });
  }

  async function remove(img) {
    const ref = img.tag === "<none>" ? img.id : `${img.repository}:${img.tag}`;
    if (!(await confirm(`Remove ${ref}?`, { danger: true, confirmLabel: "Remove" }))) return;
    await attempt(() => api.image_remove(ref, false));
  }

  async function prune(all) {
    const q = all ? "Remove every image no container uses?" : "Remove dangling (untagged) images?";
    if (!(await confirm(q, { danger: true, confirmLabel: "Remove" }))) return;
    const freed = await attempt(() => api.images_prune(all));
    if (freed != null) toast(`Freed ${bytes(freed)}`, { variant: "success" });
  }

  const total = $derived(images ? bytes([...new Map(images.map((i) => [i.id, i.size])).values()].reduce((a, b) => a + b, 0)) : "");
</script>

<div class="page">
  <header class="page-head">
    <h1>Images</h1>
    <span class="muted">{images ? `${images.length} · ${total}` : ""}</span>
    <span class="spacer"></span>
    <Button size="sm" variant="ghost" onclick={() => prune(false)}><Icon name="broom" size={14} /> Prune dangling</Button>
    <Button size="sm" variant="ghost" onclick={() => prune(true)}>Prune unused</Button>
    <Button size="sm" variant="primary" onclick={pull}><Icon name="download" size={14} /> Pull</Button>
  </header>

  {#each Object.entries(pulling) as [ref, status] (ref)}
    <Callout variant="info" title="Pulling {ref}">{status}</Callout>
  {/each}

  {#if error}
    <Callout variant="warning" title="The engine didn't answer">{error}</Callout>
  {:else if images === null}
    <Skeleton lines={6} />
  {:else}
    <Table
      rows={images}
      key="key"
      bind:query
      search
      sortable={["repository", "tag", "size", "created"]}
      label="Images"
      resizable
      persist="images"
      onrowclick={(row) => navigate(`/images/${encodeURIComponent(row.tag === "<none>" ? row.id : `${row.repository}:${row.tag}`)}`)}
      columns={[
        { key: "repository", label: "Repository" },
        { key: "tag", label: "Tag", width: 140 },
        { key: "id", label: "ID", width: 130, format: shortId },
        { key: "size", label: "Size", align: "end", width: 100, format: bytes },
        { key: "created", label: "Created", width: 120, format: ago },
        { key: "containers", label: "In use", width: 90 },
      ]}
    >
      {#snippet cell(row, column)}
        {#if column.key === "containers"}
          {#if row.containers > 0}<Badge variant="success">{row.containers}</Badge>{:else}<span class="muted">—</span>{/if}
        {:else if column.key === "id"}
          <span class="mono muted">{shortId(row.id)}</span>
        {:else}
          {column.format ? column.format(row[column.key], row) : row[column.key]}
        {/if}
      {/snippet}
      {#snippet actions(row)}
        {#if engine.status?.context === "edock"}
          <button class="icon-btn" title="Scan for vulnerabilities" onclick={() => navigate(`/images/${encodeURIComponent(row.tag === "<none>" ? row.id : `${row.repository}:${row.tag}`)}?tab=security`)}><Icon name="shield" size={14} /></button>
        {/if}
        <button class="icon-btn danger" title="Remove" onclick={() => remove(row)}><Icon name="trash" size={14} /></button>
      {/snippet}
      {#snippet empty()}<span class="muted">No images. Pull one, or `docker build`.</span>{/snippet}
    </Table>
  {/if}
</div>
