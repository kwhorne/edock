<!-- Run image…: a container from an image, as `docker run -d` makes one.
     Opened from an image, the Containers page or ⌘K (runImage()). -->
<script>
  import { channel, dialog, navigate, toast } from "@elyra/runtime";
  import { Button, Callout, Checkbox, Modal } from "@elyra/ui";
  import { onDestroy, untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import { api } from "../bindings";
  import { runner, errorText } from "./state.svelte.js";

  let image = $state("");
  let name = $state("");
  let ports = $state([]);
  let lan = $state(false);
  let mounts = $state([]);
  let env = $state([]);
  let command = $state("");
  let restart = $state(false);
  let defaults = $state(null);
  let busy = $state(false);
  let pulling = $state(null);
  let error = $state(null);
  // The image the suggestions are for, and whether the user changed the
  // ports (then a new image doesn't replace them).
  let asked = "";
  let portsTouched = false;

  // Each time it opens: fresh, for the image it was opened with.
  $effect(() => {
    if (!runner.open) return;
    const img = runner.image;
    untrack(() => {
      image = img;
      name = command = "";
      ports = [];
      mounts = [];
      env = [];
      lan = restart = busy = false;
      defaults = error = pulling = null;
      asked = "";
      portsTouched = false;
      if (img) look();
    });
  });

  async function look() {
    const img = image.trim();
    if (!img || img === asked) return;
    asked = img;
    try {
      const d = await api.image_run_defaults(img);
      if (asked !== img) return;
      defaults = d;
      if (!portsTouched) ports = d.ports.map((p) => ({ host: p.host ?? "", container: p.container, udp: p.udp }));
    } catch {
      defaults = null;
    }
  }

  const off = channel("pull").subscribe((p) => {
    if (busy && p && p.reference === image.trim()) pulling = p.done ? null : p.status || "Pulling…";
  });
  onDestroy(off);

  // Where the image keeps data and nothing is mounted yet: one click to a volume.
  const dataHints = $derived((defaults?.volumes ?? []).filter((v) => !mounts.some((m) => m.target.trim() === v)));
  function addVolume(target) {
    const base = name.trim() || defaults?.name || "data";
    const many = (defaults?.volumes.length ?? 0) > 1;
    const tail = target.split("/").filter(Boolean).pop() ?? "data";
    mounts.push({ source: many ? `${base}-${tail}` : `${base}-data`, target, read_only: false });
  }
  async function addFolder() {
    const [dir] = await dialog.open({ title: "A folder for the container", directory: true });
    if (dir) mounts.push({ source: dir.path, target: "", read_only: false });
  }

  // A pasted .env (several lines) becomes a variable each.
  function pasteEnv(e, i) {
    const text = e.clipboardData?.getData("text") ?? "";
    if (!text.includes("\n")) return;
    e.preventDefault();
    const rows = text
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith("#") && l.includes("="))
      .map((l) => {
        const at = l.indexOf("=");
        let value = l.slice(at + 1).trim();
        if (/^(["']).*\1$/.test(value)) value = value.slice(1, -1);
        return { key: l.slice(0, at).replace(/^export\s+/, "").trim(), value };
      });
    env.splice(i, 1, ...rows);
  }
  // KEY=VALUE typed or pasted in the name field: split.
  function splitEnv(row) {
    const at = row.key.indexOf("=");
    if (at > 0 && !row.value) {
      row.value = row.key.slice(at + 1);
      row.key = row.key.slice(0, at);
    }
  }

  async function run() {
    error = null;
    const p = [];
    for (const r of ports) {
      if (`${r.container}`.trim() === "" && `${r.host}`.trim() === "") continue;
      const c = Number(r.container);
      const h = `${r.host}`.trim() === "" ? null : Number(r.host);
      const bad = (n) => !Number.isInteger(n) || n < 1 || n > 65535;
      if (bad(c) || (h !== null && bad(h))) {
        error = "A port is a number from 1 to 65535.";
        return;
      }
      p.push({ host: h, container: c, udp: r.udp });
    }
    busy = true;
    try {
      const s = await api.image_run({
        image: image.trim(),
        name: name.trim() || null,
        ports: p,
        lan,
        mounts: mounts.filter((m) => m.source.trim() || m.target.trim()),
        env: env.filter((e) => e.key.trim() || e.value),
        command: command.trim() || null,
        restart,
      });
      runner.open = false;
      toast(`${s.name} is running`, { variant: "success" });
      navigate(`/containers/${s.id}`);
    } catch (e) {
      error = errorText(e);
    }
    busy = false;
    pulling = null;
  }
</script>

<Modal bind:open={runner.open} title="Run image" description="A container from an image, as docker run -d makes one." size="lg">
  <div class="run-form">
    <label class="run-field">
      <span class="muted">Image</span>
      <input class="mono" bind:value={image} onblur={look} placeholder="nginx:alpine" aria-label="Image" />
    </label>
    {#if defaults && !defaults.local}<p class="muted small run-note">It isn't here yet: Run pulls it first.</p>{/if}
    <label class="run-field">
      <span class="muted">Name</span>
      <input class="mono" bind:value={name} placeholder={defaults?.name ?? "from the image"} aria-label="Container name" />
    </label>

    <div class="run-section"><strong>Ports</strong><span class="muted small">on your Mac → in the container. Leave the Mac's empty for any free one.</span></div>
    {#each ports as p, i (i)}
      <div class="run-row">
        <input class="mono port" bind:value={p.host} oninput={() => (portsTouched = true)} placeholder="any" aria-label="Port on the Mac" />
        <span class="muted">→</span>
        <input class="mono port" bind:value={p.container} oninput={() => (portsTouched = true)} placeholder="80" aria-label="Port in the container" />
        <Checkbox bind:checked={p.udp}>UDP</Checkbox>
        <span class="spacer"></span>
        <button class="icon-btn" title="Take it out" aria-label="Take the port out" onclick={() => { ports.splice(i, 1); portsTouched = true; }}><Icon name="close" size={14} /></button>
      </div>
    {/each}
    <div class="run-add">
      <Button size="sm" variant="ghost" onclick={() => { ports.push({ host: "", container: "", udp: false }); portsTouched = true; }}><Icon name="plus" size={14} /> Port</Button>
      {#if ports.length}<Checkbox class="toggle" bind:checked={lan}>Open to other devices on the network, not only this Mac</Checkbox>{/if}
    </div>

    <div class="run-section"><strong>Folders and volumes</strong><span class="muted small">a folder from your Mac, or a volume (made if it isn't there)</span></div>
    {#each mounts as m, i (i)}
      <div class="run-row">
        <input class="mono grow" bind:value={m.source} placeholder="a volume's name, or /Users/…" aria-label="Mac folder or volume" />
        <span class="muted">→</span>
        <input class="mono grow" bind:value={m.target} placeholder="/data" aria-label="Path in the container" />
        <Checkbox bind:checked={m.read_only}>Read-only</Checkbox>
        <button class="icon-btn" title="Take it out" aria-label="Take the mount out" onclick={() => mounts.splice(i, 1)}><Icon name="close" size={14} /></button>
      </div>
    {/each}
    <div class="run-add">
      <Button size="sm" variant="ghost" onclick={addFolder}><Icon name="folder" size={14} /> Mac folder…</Button>
      <Button size="sm" variant="ghost" onclick={() => mounts.push({ source: "", target: "", read_only: false })}><Icon name="plus" size={14} /> Volume</Button>
      {#each dataHints as v (v)}
        <button class="run-hint mono" title="The image keeps data in {v}: keep it in a volume" onclick={() => addVolume(v)}>+ volume at {v}</button>
      {/each}
    </div>

    <div class="run-section"><strong>Environment</strong><span class="muted small">paste a .env to add every line</span></div>
    {#each env as e, i (i)}
      <div class="run-row">
        <input class="mono grow" bind:value={e.key} oninput={() => splitEnv(e)} onpaste={(ev) => pasteEnv(ev, i)} placeholder="KEY" aria-label="Variable" />
        <span class="muted">=</span>
        <input class="mono grow" bind:value={e.value} placeholder="value" aria-label="Value" />
        <button class="icon-btn" title="Take it out" aria-label="Take the variable out" onclick={() => env.splice(i, 1)}><Icon name="close" size={14} /></button>
      </div>
    {/each}
    <div class="run-add">
      <Button size="sm" variant="ghost" onclick={() => env.push({ key: "", value: "" })}><Icon name="plus" size={14} /> Variable</Button>
    </div>

    <label class="run-field">
      <span class="muted">Command</span>
      <input class="mono" bind:value={command} placeholder={defaults?.command ?? "the image's own"} aria-label="Command" />
    </label>
    <Checkbox class="toggle" bind:checked={restart}>Start it again when the engine starts (restart unless stopped)</Checkbox>

    {#if pulling}<Callout variant="info">Pulling {image.trim()}: {pulling}</Callout>{/if}
    {#if error}<Callout variant="danger" title="It didn't run">{error}</Callout>{/if}
  </div>
  {#snippet footer()}
    <Button onclick={() => (runner.open = false)}>Cancel</Button>
    <Button variant="primary" disabled={!image.trim()} loading={busy} onclick={run}>Run</Button>
  {/snippet}
</Modal>
