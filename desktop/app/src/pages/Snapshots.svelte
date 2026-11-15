<!-- Snapshots of containers and volumes: back to how they were. -->
<script>
  import { confirm, navigate, toast } from "@elyra/runtime";
  import { Button, Callout, Empty, Skeleton } from "@elyra/ui";
  import { api } from "../bindings";
  import Icon from "../lib/Icon.svelte";
  import { ago, bytes } from "../lib/format.js";
  import { attempt, changes } from "../lib/state.svelte.js";

  let list = $state(null);
  let error = $state(null);
  let busy = $state(null);

  async function load() {
    try {
      list = await api.snapshots_list();
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }
  $effect(() => {
    changes.container;
    changes.volume;
    changes.image;
    load();
  });

  const containers = $derived(list?.filter((s) => s.kind === "container") ?? []);
  const volumes = $derived(list?.filter((s) => s.kind === "volume") ?? []);
  const key = (s) => `${s.kind}/${s.of}/${s.id}`;

  async function restore(s) {
    if (s.kind === "container") {
      const ok = await confirm(
        `Make ${s.of} again from this snapshot? If it's running it's stopped first; it's kept, renamed ${s.of}-before-…, until you remove it. Its volumes are left as they are.`,
        { confirmLabel: "Restore" },
      );
      if (!ok) return;
      busy = key(s);
      const r = await attempt(() => api.container_snapshot_restore(s.id));
      busy = null;
      if (r) toast(r.previous ? `${r.container} restored. The one before is kept ${r.previous.startsWith("edock-snapshots/") ? "as a snapshot" : "as " + r.previous}.` : `${r.container} made from the snapshot`, { variant: "success", duration: 6000 });
    } else {
      const ok = await confirm(`Put ${s.of} back as it was ${ago(s.created)}? What's in it now is replaced. Containers using it must be stopped.`, {
        danger: true,
        confirmLabel: "Restore",
      });
      if (!ok) return;
      busy = key(s);
      await attempt(() => api.volume_snapshot_restore(s.of, s.id), `${s.of} restored`);
      busy = null;
    }
    load();
  }

  async function remove(s) {
    if (!(await confirm(`Delete this snapshot of ${s.of}?`, { danger: true, confirmLabel: "Delete" }))) return;
    await attempt(() => api.snapshot_delete(s.kind, s.of, s.id));
    load();
  }
</script>

{#snippet rows(items)}
  <ul class="rows snapshot-rows">
    {#each items as s (key(s))}
      <li class="snapshot">
        <div class="main">
          <div class="name">{s.of}</div>
          <div class="sub muted">
            {ago(s.created)} · {bytes(s.size)}{#if s.note} · {s.note}{/if}
          </div>
        </div>
        <span class="muted mono sub" title={s.id}>{s.kind === "container" ? s.id.split(":").pop() : s.id.replace(".tar.gz", "")}</span>
        <div class="chips">
          <Button size="sm" loading={busy === key(s)} onclick={() => restore(s)}>Restore</Button>
          <button class="icon-btn danger" title="Delete" onclick={() => remove(s)}><Icon name="trash" size={14} /></button>
        </div>
      </li>
    {/each}
  </ul>
{/snippet}

<div class="page">
  <header class="page-head">
    <h1>Snapshots</h1>
    <span class="muted">{list ? list.length : ""}</span>
  </header>

  {#if error}
    <Callout variant="warning" title="Couldn't read the snapshots">{error}</Callout>
  {:else if list === null}
    <Skeleton lines={5} />
  {:else if list.length === 0}
    <Empty
      title="No snapshots yet"
      description="Take one before trying something risky: a container's files with Snapshot on its page, a volume's with the camera on the Volumes page. Restore puts it back as it was."
    >
      {#snippet action()}
        <Button onclick={() => navigate("/")}>Containers</Button>
        <Button onclick={() => navigate("/volumes")}>Volumes</Button>
      {/snippet}
    </Empty>
  {:else}
    <section class="group">
      <div class="group-head"><strong>Containers</strong><span class="muted">{containers.length}</span></div>
      {#if containers.length}
        {@render rows(containers)}
      {:else}
        <p class="muted pad">None. <strong>Snapshot</strong> on a container's page takes one of its files.</p>
      {/if}
    </section>
    <section class="group">
      <div class="group-head"><strong>Volumes</strong><span class="muted">{volumes.length}</span></div>
      {#if volumes.length}
        {@render rows(volumes)}
      {:else}
        <p class="muted pad">None. The camera on the Volumes page takes one.</p>
      {/if}
    </section>
    <p class="muted small">
      A container's snapshot is an image (<span class="mono">edock-snapshots/…</span>) of its files; its volumes aren't in it. A volume's is a
      <span class="mono">.tar.gz</span> in <span class="mono">~/.edock/snapshots/volumes</span> on your Mac.
    </p>
  {/if}
</div>
