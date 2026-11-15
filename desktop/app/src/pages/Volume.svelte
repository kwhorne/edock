<!-- One volume: its files (through a short-lived helper), and what to do with it. -->
<script>
  import { confirm, dialog, navigate, prompt, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import Files from "../lib/Files.svelte";
  import Icon from "../lib/Icon.svelte";
  import { bytes } from "../lib/format.js";
  import { attempt } from "../lib/state.svelte.js";

  let { params } = $props();
  const name = $derived(decodeURIComponent(params.name));
  let info = $state(null);
  let helper = $state(null);
  let error = $state(null);

  $effect(() => {
    const n = name;
    api.volumes_list().then((all) => (info = all.find((v) => v.name === n) ?? null));
    helper = null;
    error = null;
    api
      .volume_browse(n)
      .then((id) => (helper = id))
      .catch((e) => (error = e?.message ?? String(e)));
    return () => api.volume_browse_end(n).catch(() => {});
  });
  onDestroy(() => api.volume_browse_end(name).catch(() => {}));

  async function snapshot() {
    const note = await prompt(`Snapshot ${name}'s files. A note, if you like:`, { placeholder: "before the migration", confirmLabel: "Take snapshot" });
    if (note != null) await attempt(() => api.volume_snapshot(name, note.trim() || null), `Snapshot of ${name} taken`);
  }
  async function clone() {
    const target = await prompt(`Copy ${name} into a new volume:`, { defaultValue: `${name}-copy`, confirmLabel: "Clone" });
    if (target?.trim()) {
      const done = await attempt(() => api.volume_clone(name, target.trim()).then(() => true), `${target.trim()} is a copy of ${name}`);
      if (done) navigate(`/volumes/${encodeURIComponent(target.trim())}`);
    }
  }
  async function exportIt() {
    const file = await dialog.save({ title: `Export ${name}`, defaultName: `${name}.tar` });
    if (file) await attempt(() => api.volume_export(name, file), `${name} saved to ${file.name}`);
  }
  async function remove() {
    if (!(await confirm(`Remove ${name} and everything in it?`, { danger: true, confirmLabel: "Remove" }))) return;
    if (await attempt(() => api.volume_remove(name).then(() => true))) {
      toast(`Removed ${name}`, { variant: "success" });
      navigate("/volumes");
    }
  }
</script>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/volumes")}><Icon name="back" /></button>
    <h1 class="mono">{name}</h1>
    {#if info?.size != null}<span class="muted">{bytes(info.size)}</span>{/if}
    <span class="spacer"></span>
    <Button size="sm" onclick={snapshot} title="Snapshot its files"><Icon name="camera" size={14} /> Snapshot</Button>
    <Button size="sm" onclick={clone}><Icon name="copy" size={14} /> Clone</Button>
    <Button size="sm" onclick={exportIt}><Icon name="download" size={14} /> Export</Button>
    <Button size="sm" variant="danger" onclick={remove}><Icon name="trash" size={14} /></Button>
  </header>

  {#if info}
    <div class="facts">
      <div><span class="muted">Used by</span>{#if info.used_by.length}{#each info.used_by as c (c)}<Badge variant="accent">{c}</Badge> {/each}{:else}<span class="muted">nothing</span>{/if}</div>
      {#if info.project}<div><span class="muted">Project</span>{info.project}</div>{/if}
      <div><span class="muted">Driver</span>{info.driver}</div>
    </div>
  {/if}

  {#if error}
    <Callout variant="warning" title="Couldn't open its files">{error}</Callout>
  {:else if !helper}
    <Skeleton lines={6} />
  {:else}
    <p class="muted small">
      Its files, through a small helper container eDock removes when you leave. Changes here are changes to the volume; a container writing to it at the same time sees them.
    </p>
    <Files container={helper} running={true} root="/v" rootName={name} />
  {/if}
</div>
