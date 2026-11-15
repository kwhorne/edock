<script>
  import { channel, clipboard, toast } from "@elyra/runtime";
  import { Input, Switch } from "@elyra/ui";
  import { onDestroy, tick } from "svelte";
  import { api } from "../bindings";

  // A container's log, or what `start(tail, timestamps)` opens (a pod's).
  let { container = null, start: open = null } = $props();

  const MAX = 5000;
  let lines = $state([]);
  let ended = $state(false);
  let filter = $state("");
  let timestamps = $state(false);
  let wrap = $state(true);
  let follow = $state(true);
  let box;

  let streamId = null;
  let unsub = null;
  let seq = 0;

  async function start() {
    stop();
    lines = [];
    ended = false;
    const mine = ++seq;
    const id = open ? await open(500, timestamps) : await api.logs_follow(container, 500, timestamps);
    if (mine !== seq) return api.stream_stop(id);
    streamId = id;
    unsub = channel(`logs:${id}`).subscribe(async (batch) => {
      if (!batch) return;
      const next = lines.concat(batch.lines);
      lines = next.length > MAX ? next.slice(next.length - MAX) : next;
      if (batch.ended) ended = true;
      if (follow) {
        await tick();
        box?.scrollTo({ top: box.scrollHeight });
      }
    });
  }

  function stop() {
    // An open still on its way finds seq moved on, and closes what it got.
    seq++;
    unsub?.();
    unsub = null;
    if (streamId != null) api.stream_stop(streamId);
    streamId = null;
  }

  $effect(() => {
    container;
    timestamps;
    start();
  });
  onDestroy(stop);

  function onScroll() {
    if (!box) return;
    follow = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
  }

  // A project's combined log: lines carry their service, each in its colour,
  // and services can be hidden.
  let hidden = $state({});
  const services = $derived([...new Set(lines.map((l) => l.service).filter(Boolean))].sort());
  const PALETTE = ["#4f9cf9", "#e8a33d", "#3fb68b", "#d36ac2", "#e5654e", "#8c7ae6", "#2bb5c4", "#b8a03a"];
  const colour = (svc) => PALETTE[[...svc].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) % PALETTE.length];

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return lines.filter((l) => !(l.service && hidden[l.service]) && (!q || l.text.toLowerCase().includes(q) || l.service?.toLowerCase().includes(q)));
  });

  // ANSI colour codes off: the log is shown as text.
  const plain = (s) => s.replace(/\x1b\[[0-9;?]*[A-Za-z]/g, "");
</script>

<div class="logs">
  <div class="logs-bar">
    <Input bind:value={filter} placeholder="Filter lines" aria-label="Filter log lines" />
    <label class="toggle"><Switch bind:checked={timestamps} /> Timestamps</label>
    <label class="toggle"><Switch bind:checked={wrap} /> Wrap</label>
    <span class="spacer"></span>
    <span class="muted">{shown.length} lines{ended ? " · ended" : ""}</span>
    <button class="ghost-btn" onclick={() => clipboard.writeText(shown.map((l) => (l.service ? `${l.service} | ` : "") + plain(l.text)).join("\n")).then(() => toast("Copied"))}>Copy</button>
    <button class="ghost-btn" onclick={() => (lines = [])}>Clear</button>
  </div>
  {#if services.length > 1}
    <div class="chips">
      {#each services as svc (svc)}
        <button class="svc-chip" class:off={hidden[svc]} style="--svc: {colour(svc)}" onclick={() => (hidden[svc] = !hidden[svc])} title={hidden[svc] ? `Show ${svc}` : `Hide ${svc}`}>{svc}</button>
      {/each}
    </div>
  {/if}
  <div class="log-box" class:wrap bind:this={box} onscroll={onScroll}>
    {#each shown as l, i (i)}
      <div class="line" class:err={l.stream === "err"}>{#if l.service}<span class="svc" style="color: {colour(l.service)}">{l.service}</span>{/if}{plain(l.text)}</div>
    {/each}
    {#if lines.length === 0}
      <div class="muted">{ended ? "No output." : "Waiting for output…"}</div>
    {/if}
  </div>
  {#if !follow}
    <button class="follow-btn" onclick={() => { follow = true; box.scrollTo({ top: box.scrollHeight }); }}>↓ Follow</button>
  {/if}
</div>
