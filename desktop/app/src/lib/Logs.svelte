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

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return q ? lines.filter((l) => l.text.toLowerCase().includes(q)) : lines;
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
    <button class="ghost-btn" onclick={() => clipboard.writeText(shown.map((l) => plain(l.text)).join("\n")).then(() => toast("Copied"))}>Copy</button>
    <button class="ghost-btn" onclick={() => (lines = [])}>Clear</button>
  </div>
  <div class="log-box" class:wrap bind:this={box} onscroll={onScroll}>
    {#each shown as l, i (i)}
      <div class="line" class:err={l.stream === "err"}>{plain(l.text)}</div>
    {/each}
    {#if lines.length === 0}
      <div class="muted">{ended ? "No output." : "Waiting for output…"}</div>
    {/if}
  </div>
  {#if !follow}
    <button class="follow-btn" onclick={() => { follow = true; box.scrollTo({ top: box.scrollHeight }); }}>↓ Follow</button>
  {/if}
</div>
