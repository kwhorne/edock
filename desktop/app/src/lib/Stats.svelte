<script>
  import { channel } from "@elyra/runtime";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import { bytes, percent } from "./format.js";

  let { container } = $props();

  const N = 60;
  let points = $state([]);
  let streamId = null;
  let unsub = null;
  let seq = 0;

  async function start() {
    stop();
    points = [];
    const mine = ++seq;
    const id = await api.stats_follow(container);
    if (mine !== seq) return api.stream_stop(id);
    streamId = id;
    let prev = null;
    unsub = channel(`stats:${id}`).subscribe((p) => {
      if (!p) return;
      const t = performance.now();
      // Network and disk come as totals: rates from the last point.
      const dt = prev ? (t - prev.t) / 1000 : 0;
      const rate = (k) => (prev && dt > 0 ? Math.max(0, (p[k] - prev.p[k]) / dt) : 0);
      const point = { ...p, rx: rate("net_rx"), tx: rate("net_tx"), rd: rate("block_read"), wr: rate("block_write") };
      prev = { t, p };
      points = points.length >= N ? [...points.slice(1), point] : [...points, point];
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
    start();
  });
  onDestroy(stop);

  const last = $derived(points.at(-1));

  function path(values, max) {
    if (values.length < 2) return "";
    const m = Math.max(max ?? 0, ...values, 1e-9);
    const w = 300;
    const h = 60;
    const step = w / (N - 1);
    const off = (N - values.length) * step;
    return values.map((v, i) => `${i ? "L" : "M"}${(off + i * step).toFixed(1)},${(h - (v / m) * (h - 4) - 2).toFixed(1)}`).join(" ");
  }

  const cards = $derived([
    {
      title: "CPU",
      value: last ? percent(last.cpu) : "—",
      sub: last ? `of ${last.cpus * 100}% (${last.cpus} CPUs)` : "",
      series: [points.map((p) => p.cpu)],
      max: 100,
    },
    {
      title: "Memory",
      value: last ? bytes(last.memory) : "—",
      sub: last && last.memory_limit ? `of ${bytes(last.memory_limit)}` : "",
      series: [points.map((p) => p.memory)],
    },
    {
      title: "Network",
      value: last ? `↓ ${bytes(last.rx)}/s  ↑ ${bytes(last.tx)}/s` : "—",
      sub: last ? `${bytes(last.net_rx)} in · ${bytes(last.net_tx)} out` : "",
      series: [points.map((p) => p.rx), points.map((p) => p.tx)],
    },
    {
      title: "Disk",
      value: last ? `R ${bytes(last.rd)}/s  W ${bytes(last.wr)}/s` : "—",
      sub: last ? `${bytes(last.block_read)} read · ${bytes(last.block_write)} written · ${last.pids} processes` : "",
      series: [points.map((p) => p.rd), points.map((p) => p.wr)],
    },
  ]);
</script>

<div class="stats">
  {#each cards as c (c.title)}
    {@const max = c.max ?? Math.max(...c.series.flat(), 0) * 1.25}
    <div class="stat-card">
      <div class="stat-title muted">{c.title}</div>
      <div class="stat-value">{c.value}</div>
      <div class="stat-sub muted">{c.sub}</div>
      <svg viewBox="0 0 300 60" preserveAspectRatio="none" class="spark" aria-hidden="true">
        {#each c.series as s, i (i)}
          <path d={path(s, max)} class:second={i === 1} />
        {/each}
      </svg>
    </div>
  {/each}
</div>
