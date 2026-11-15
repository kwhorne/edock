<!-- A container's files: browse, preview, save to the Mac, upload, remove. -->
<script>
  import { clipboard, confirm, dialog, onFileDrop, toast } from "@elyra/runtime";
  import { Button, Callout, Empty, Skeleton } from "@elyra/ui";
  import { onDestroy, untrack } from "svelte";
  import { api } from "../bindings";
  import Icon from "./Icon.svelte";
  import { ago, bytes } from "./format.js";
  import { attempt, engine, errorText } from "./state.svelte.js";

  // `root`: where it starts, and the top it can't go above (a volume's
  // helper has it at /v, shown as the volume's top).
  let { container, running, root = "/", rootName = null } = $props();

  let path = $state(untrack(() => root));
  let entries = $state(null);
  let error = $state(null);
  let open = $state(null); // { path, entry, preview }
  let busy = $state(false);
  const onEdock = $derived(engine.status?.context === "edock");

  const join = (dir, name) => (dir.endsWith("/") ? dir + name : `${dir}/${name}`);
  const parent = (p) => (p === root ? root : p.replace(/\/[^/]+\/?$/, "") || "/");
  const shown = (p) => (root === "/" ? p : p.slice(root.length) || "/");
  const crumbs = $derived(
    shown(path)
      .split("/")
      .filter(Boolean)
      .map((part, i, all) => ({ part, path: (root === "/" ? "" : root) + "/" + all.slice(0, i + 1).join("/") })),
  );

  // Only the last folder asked for is shown, whichever answers first.
  let asked = 0;
  async function load(to = path) {
    const mine = ++asked;
    try {
      const list = await api.container_files(container, to);
      if (mine !== asked) return;
      entries = list;
      path = to;
      error = null;
    } catch (e) {
      if (mine !== asked) return;
      error = errorText(e);
      entries = null;
    }
  }
  // Another container or root (a volume cloned and opened): from its top.
  // The same one starting or stopping: the folder again.
  let shownFor = null;
  $effect(() => {
    const key = `${container}\u0000${root}`;
    running;
    untrack(() => {
      if (key !== shownFor) {
        shownFor = key;
        path = root;
        open = null;
      }
      load(path);
    });
  });

  async function select(e) {
    const p = join(path, e.name);
    if (e.kind === "dir") {
      open = null;
      return load(p);
    }
    open = { path: p, entry: e, preview: null };
    try {
      const preview = await api.container_file_preview(container, p);
      if (open?.path === p) open = { ...open, preview };
    } catch (err) {
      if (open?.path === p) open = { ...open, error: errorText(err) };
    }
  }

  // A path with its `.` and `..` worked out.
  function normal(p) {
    const out = [];
    for (const part of p.split("/")) {
      if (part === "" || part === ".") continue;
      if (part === "..") out.pop();
      else out.push(part);
    }
    return "/" + out.join("/");
  }

  // A link: go where it points, a folder or a file, but never above the
  // root (a volume's link to /etc is the helper's /etc, not the volume's).
  async function follow(target) {
    const to = normal(target.startsWith("/") ? (root === "/" ? target : root + target) : join(parent(open.path), target));
    if (root !== "/" && to !== root && !to.startsWith(root + "/")) {
      toast("That link points outside it", { variant: "error" });
      return;
    }
    // The root itself (a link to the volume's top): there, not a child named like it.
    if (to === root) {
      open = null;
      return load(root);
    }
    const dir = parent(to);
    await load(dir);
    const e = entries?.find((x) => x.name === to.split("/").pop());
    if (e) select(e);
  }

  async function save(p, kind) {
    const name = p.split("/").pop() || "root";
    const file = await dialog.save({ title: kind === "dir" ? `Save ${name} as a .tar` : `Save ${name}`, defaultName: kind === "dir" ? `${name}.tar` : name });
    if (!file) return;
    await attempt(() => api.container_file_save(container, p, file), `Saved ${file.name}`);
  }

  async function remove(p) {
    if (!(await confirm(`Remove ${shown(p)}? This can't be undone.`, { danger: true, confirmLabel: "Remove" }))) return;
    await attempt(() => api.container_file_remove(container, p), `Removed ${shown(p)}`);
    if (open?.path === p) open = null;
    load();
  }

  async function upload(files) {
    if (!files?.length) return;
    busy = true;
    const n = await attempt(() => api.container_files_upload(container, path, files));
    busy = false;
    if (n != null) toast(`${n} file${n === 1 ? "" : "s"} put in ${shown(path)}`, { variant: "success" });
    load();
  }
  const pick = async () => upload(await dialog.open({ title: `Put files in ${shown(path)}`, multiple: true }));
  // Files dropped on the window while this tab shows go into this folder.
  const stopDrop = onFileDrop((files) => running && upload(files));
  onDestroy(stopDrop);

  function rwx(mode) {
    let s = "";
    for (const shift of [6, 3, 0]) {
      const b = (mode >> shift) & 7;
      s += (b & 4 ? "r" : "-") + (b & 2 ? "w" : "-") + (b & 1 ? "x" : "-");
    }
    return s;
  }
  const icon = (k) => (k === "dir" ? "folder" : k === "link" ? "external" : "file");
</script>

<div class="files">
  <div class="files-bar">
    <button class="icon-btn" title="Up" disabled={path === root} onclick={() => load(parent(path))}><Icon name="back" size={14} /></button>
    <nav class="crumbs mono" aria-label="Path">
      <button class="crumb" onclick={() => load(root)}>{rootName ? `${rootName}:/` : "/"}</button>
      {#each crumbs as c, i (c.path)}
        <button class="crumb" onclick={() => load(c.path)}>{c.part}</button>{#if i < crumbs.length - 1}<span class="muted">/</span>{/if}
      {/each}
    </nav>
    <span class="spacer"></span>
    <button class="icon-btn" title="Copy the path" onclick={() => clipboard.writeText(shown(path)).then(() => toast("Copied"))}><Icon name="copy" size={14} /></button>
    <button class="icon-btn" title="Refresh" onclick={() => load()}><Icon name="restart" size={14} /></button>
    {#if running}<Button size="sm" loading={busy} onclick={pick}><Icon name="upload" size={14} /> Put files here</Button>{/if}
  </div>

  {#if error}
    <Callout variant={running ? "warning" : "info"} title={running ? "Couldn't list this folder" : "The container isn't running"}>
      {error}
      {#if path !== root && running}<div class="callout-action"><Button size="sm" onclick={() => load(root)}>Go to the top</Button></div>{/if}
    </Callout>
  {:else if !entries}
    <Skeleton lines={6} />
  {:else}
    <div class="files-body" class:open={open != null}>
      <ul class="file-list" aria-label="Files">
        {#each entries as e (e.name)}
          <li class="file-row" class:selected={open?.path === join(path, e.name)}>
            <button class="file-main" onclick={() => select(e)} title={join(path, e.name)}>
              <span class="file-icon" class:dir={e.kind === "dir"}><Icon name={icon(e.kind)} size={14} /></span>
              <span class="file-name mono">{e.name}{e.kind === "dir" ? "/" : ""}</span>
              <span class="muted mono perm">{rwx(e.mode)}</span>
              <span class="muted num">{e.kind === "file" ? bytes(e.size) : ""}</span>
              <span class="muted when">{e.mtime ? ago(e.mtime) : ""}</span>
            </button>
            <div class="row-actions">
              <button class="icon-btn" title={e.kind === "dir" ? "Save as a .tar" : "Save to the Mac"} onclick={() => save(join(path, e.name), e.kind)}><Icon name="download" size={13} /></button>
              {#if running}<button class="icon-btn danger" title="Remove" onclick={() => remove(join(path, e.name))}><Icon name="trash" size={13} /></button>{/if}
            </div>
          </li>
        {:else}
          <Empty title="Empty folder" description={running ? "Drop files on the window, or Put files here." : ""} />
        {/each}
      </ul>

      {#if open}
        <section class="file-preview">
          <div class="detail-head">
            <span class="mono detail-url">{shown(open.path)}</span>
            <span class="spacer"></span>
            <button class="icon-btn" title="Close" onclick={() => (open = null)}>✕</button>
          </div>
          <div class="chips">
            <Button size="sm" onclick={() => save(open.path, open.entry.kind)}>Save to the Mac</Button>
            <Button size="sm" variant="ghost" onclick={() => clipboard.writeText(shown(open.path)).then(() => toast("Copied"))}>Copy path</Button>
            {#if onEdock && running && root === "/"}<Button size="sm" variant="ghost" onclick={() => attempt(() => api.container_file_reveal(container, open.path))}>Show in Finder</Button>{/if}
          </div>
          {#if open.error}
            <Callout variant="warning">{open.error}</Callout>
          {:else if !open.preview}
            <Skeleton lines={6} />
          {:else if open.preview.link}
            <p>A link to <span class="mono">{open.preview.link}</span></p>
            <div><Button size="sm" onclick={() => follow(open.preview.link)}>Go there</Button></div>
          {:else if open.preview.binary}
            <p class="muted">Binary, {bytes(open.preview.size)}. Save it to look at it on the Mac.</p>
          {:else}
            <p class="muted">{bytes(open.preview.size)}{open.preview.cut ? " · the first 256 KB" : ""}</p>
            <pre class="raw body">{open.preview.text}</pre>
          {/if}
        </section>
      {/if}
    </div>
  {/if}
</div>
