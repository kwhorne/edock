<!-- The HTTP inspector: requests through *.edock.localhost, what the
     containers answered, and what they wrote to their logs meanwhile. -->
<script>
  import { clipboard, shell, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Empty, Input, Select, Switch, Tabs } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import Icon from "../lib/Icon.svelte";
  import { bytes } from "../lib/format.js";
  import { attempt, engine, useContext, errorText } from "../lib/state.svelte.js";

  const onEdock = $derived(engine.status?.context === "edock" && engine.status?.connected);

  let on = $state(false);
  let loaded = $state(false);
  let error = $state(null);
  // seq -> row; the engine sends what changed since `version`.
  let rows = $state(new Map());
  let version = 0;
  let search = $state("");
  let status = $state("all");
  let container = $state("all");
  let selected = $state(null);
  let detail = $state(null);
  let tab = $state("headers");
  // What the containers wrote while the open request was handled.
  let during = $state(null);
  let duringError = $state(null);

  async function load() {
    if (!onEdock) return;
    try {
      const log = await api.http_requests(version);
      on = log.on;
      if (log.version < version) rows = new Map(); // the engine restarted
      const next = new Map(rows);
      for (const r of log.items) next.set(r.seq, r);
      if (log.first == null && log.items.length === 0 && log.version !== version) next.clear();
      if (log.first != null) for (const k of next.keys()) if (k < log.first) next.delete(k);
      rows = next;
      version = log.version;
      error = null;
      // The open one changed (its response arrived): read it again.
      if (selected != null && log.items.some((r) => r.seq === selected)) openRow(selected, true);
    } catch (e) {
      error = errorText(e);
    }
    loaded = true;
  }
  $effect(() => {
    if (onEdock) load();
  });
  const timer = setInterval(() => !document.hidden && load(), 1000);
  onDestroy(() => clearInterval(timer));

  async function record(v) {
    await attempt(() => api.http_record(v), v ? "Recording requests" : "Stopped recording");
    load();
  }
  async function clear() {
    await attempt(() => api.http_clear());
    rows = new Map();
    selected = null;
    detail = null;
    during = null;
    duringError = null;
    load();
  }
  // A request or response without a body has no tab for it: its headers.
  $effect(() => {
    if (detail && ((tab === "request" && !detail.req_body) || (tab === "response" && !detail.res_body))) tab = "headers";
  });

  async function openRow(seq, quiet = false) {
    if (!quiet) {
      selected = seq;
      detail = null;
      during = null;
      duringError = null;
    }
    try {
      const d = await api.http_request(seq);
      if (selected !== seq) return;
      detail = d;
      // A failed one opens on what the containers said about it.
      if (!quiet && (d.row.error || (d.row.status ?? 0) >= 500)) tab = "logs";
      loadDuring(seq);
    } catch (e) {
      if (!quiet) toast(errorText(e), { variant: "error" });
    }
  }
  async function loadDuring(seq) {
    try {
      const l = await api.http_request_logs(seq);
      if (selected === seq) {
        during = l;
        duringError = null;
      }
    } catch (e) {
      if (selected === seq) duringError = errorText(e);
    }
  }
  // +12 ms, −300 ms, +1.20 s: from the request's arrival.
  const offset = (s) => {
    const ms = s * 1000;
    const sign = ms < 0 ? "−" : "+";
    const a = Math.abs(ms);
    return `${sign}${a < 1000 ? `${Math.round(a)} ms` : `${(a / 1000).toFixed(2)} s`}`;
  };
  // Before it arrived, or after its response ended: the window's margins.
  const outside = (l, row) => l.offset < 0 || (row.ms != null && l.offset * 1000 > row.ms);
  const who = (l) => l.service ?? l.container;

  // The exchange and its logs as Markdown, for an AI assistant (or a bug
  // report). Secrets are left out: headers that carry them, and the values
  // of query parameters named like them, in the URL and in the headers
  // that hold one (an OAuth redirect's ?code=, a signed URL's signature).
  const SECRET = /^(authorization|proxy-authorization|cookie|set-cookie)$|token|secret|signature|api-?key|passw|session/i;
  const SECRET_PARAM = /token|secret|passw|pwd|signature|credential|session|jwt|auth|apikey|(^|[_-])(key|sig|code)$|^(key|sig|code)([_-]|$)/i;
  const URL_HEADER = /^(location|content-location|referer|origin)$/i;
  const hideQuery = (u) =>
    u.replace(/([?&])([^=&#]+)=([^&#]*)/g, (m, sep, k) => {
      let name = k;
      try {
        name = decodeURIComponent(k);
      } catch {}
      return SECRET_PARAM.test(name) ? `${sep}${k}=[hidden]` : m;
    });
  function forAi(d, l) {
    const fence = "```";
    const cap = (t) => (t && t.length > 4000 ? `${t.slice(0, 4000)}\n… (cut)` : t || "");
    const hs = (h) => h.map(([k, v]) => `${k}: ${SECRET.test(k) ? "[hidden]" : URL_HEADER.test(k) ? hideQuery(v) : v}`).join("\n");
    const out = [`## ${d.row.method} ${hideQuery(url(d.row))} → ${d.row.error ? `failed: ${d.row.error}` : `${d.row.status ?? "no answer"} ${d.row.reason}`} (${took(d.row.ms)})`, "", `Answered by the container \`${d.row.container}\`.` + (d.row.woke_ms != null ? ` It was parked (On demand): the request waited ${took(d.row.woke_ms)} for it to wake.` : "")];
    out.push("", "### Request headers", fence, hs(d.req_headers), fence);
    if (d.req_body?.text) out.push("", "### Request body", fence, cap(d.req_body.text), fence);
    if (d.res_headers.length) out.push("", "### Response headers", fence, hs(d.res_headers), fence);
    if (d.res_body?.text) out.push("", "### Response body", fence, cap(pretty(d.res_body)), fence);
    if (l) {
      out.push("", `### Container logs from ${BEFORE_S} s before to ${AFTER_S} s after (${l.containers.join(", ")})`, fence);
      out.push(l.lines.length ? l.lines.map((x) => `${offset(x.offset)} ${who(x)}${x.stderr ? " (stderr)" : ""}: ${x.text}`).join("\n") : "(nothing)");
      out.push(fence);
    }
    return out.join("\n");
  }
  const BEFORE_S = 0.5;
  const AFTER_S = 1;

  const all = $derived([...rows.values()].reverse());
  const containers = $derived([...new Set(all.map((r) => r.container))].sort());
  const CLASSES = { all: () => true, "2xx": (s) => s >= 200 && s < 300, "3xx": (s) => s >= 300 && s < 400, "4xx": (s) => s >= 400 && s < 500, "5xx": (s) => s >= 500 };
  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    return all.filter((r) => {
      if (container !== "all" && r.container !== container) return false;
      if (status === "failed") {
        if (!(r.error || (r.status ?? 0) >= 400)) return false;
      } else if (status !== "all" && !(r.status != null && CLASSES[status](r.status))) return false;
      return !q || `${r.method} ${r.host}${r.url} ${r.container} ${r.status ?? ""}`.toLowerCase().includes(q);
    });
  });

  const tone = (r) => (r.error ? "danger" : r.status == null ? "neutral" : r.status >= 500 ? "danger" : r.status >= 400 ? "warning" : r.status >= 300 ? "accent" : "success");
  const time = (at) => new Date(at * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
  const took = (ms) => (ms == null ? "…" : ms < 1000 ? `${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms` : `${(ms / 1000).toFixed(2)} s`);
  const kind = (t) => (t || "").split(";")[0].replace(/^application\//, "").replace(/^text\//, "") || "";
  const url = (r) => `${r.scheme}://${r.host}${r.url}`;

  // A body as it reads best: JSON indented.
  function pretty(body) {
    if (!body?.text) return "";
    const t = body.text.trim();
    if (!body.cut && (t.startsWith("{") || t.startsWith("["))) {
      try {
        return JSON.stringify(JSON.parse(t), null, 2);
      } catch {}
    }
    return body.text;
  }

  const q = (s) => `'${s.replace(/'/g, `'\\''`)}'`;
  function curl(d) {
    const skip = new Set(["host", "content-length", "connection", "accept-encoding"]);
    const parts = [`curl -X ${d.row.method} ${q(url(d.row))}`];
    if (d.row.scheme === "https") parts[0] += " -k";
    for (const [k, v] of d.req_headers) if (!skip.has(k.toLowerCase())) parts.push(`-H ${q(`${k}: ${v}`)}`);
    if (d.req_body?.text) parts.push(`--data-raw ${q(d.req_body.text)}`);
    return parts.join(" \\\n  ");
  }

  async function replay(d) {
    await attempt(() => api.http_replay(d.row.seq), `${d.row.method} ${d.row.url} sent again`);
    load();
  }
</script>

<div class="page requests-page">
  <header class="page-head">
    <h1>Requests</h1>
    <span class="muted">{rows.size || ""}</span>
    <span class="spacer"></span>
    {#if onEdock}
      <label class="toggle" title="Record requests through *.edock.localhost"><Switch checked={on} onchange={record} /> Record</label>
      <Button size="sm" variant="ghost" onclick={clear} disabled={rows.size === 0}>Clear</Button>
    {/if}
  </header>

  {#if !onEdock}
    <Callout variant="info" title="The HTTP inspector is eDock's engine's">
      It shows requests to containers through <span class="mono">*.edock.localhost</span>. This window shows <strong>{engine.status?.context}</strong>.
      <div class="callout-action"><Button size="sm" onclick={() => useContext("edock")}>Switch to eDock</Button></div>
    </Callout>
  {:else if error}
    <Callout variant="warning" title="Couldn't read the requests">{error}</Callout>
  {:else if loaded && rows.size === 0}
    <Empty
      title={on ? "Waiting for requests" : "See every request to your containers"}
      description={on
        ? "Open a container's domain, http://<name>.edock.localhost, and its requests show here as they happen."
        : "While recording, each request through http(s)://<name>.edock.localhost shows here with its headers, body, status and timing. Nothing is changed on the way, and nothing is saved to disk."}
    >
      {#snippet action()}
        {#if !on}<Button variant="primary" onclick={() => record(true)}>Start recording</Button>{/if}
      {/snippet}
    </Empty>
  {:else if loaded}
    <div class="requests-bar">
      <div class="search"><Icon name="search" size={14} /><Input bind:value={search} placeholder="Filter" aria-label="Filter requests" /></div>
      <Select
        value={status}
        onchange={(v) => (status = v)}
        options={[
          { value: "all", label: "All statuses" },
          { value: "2xx", label: "2xx" },
          { value: "3xx", label: "3xx" },
          { value: "4xx", label: "4xx" },
          { value: "5xx", label: "5xx" },
          { value: "failed", label: "Failed (4xx, 5xx, no answer)" },
        ]}
      />
      <Select value={container} onchange={(v) => (container = v)} options={[{ value: "all", label: "All containers" }, ...containers.map((c) => ({ value: c, label: c }))]} />
      {#if !on}<Badge variant="warning">Not recording</Badge>{/if}
    </div>

    <div class="requests" class:open={selected != null}>
      <div class="request-list" role="listbox" aria-label="Requests">
        {#each shown as r (r.seq)}
          <button class="request" class:selected={r.seq === selected} role="option" aria-selected={r.seq === selected} onclick={() => openRow(r.seq)}>
            <span class="muted mono when">{time(r.at)}</span>
            <span class="mono method">{r.method}</span>
            <span class="target">
              <span class="mono path">{r.url}</span>
              <span class="sub muted">{r.container}{r.scheme === "https" ? " · https" : ""}{r.woke_ms != null ? ` · woke it in ${took(r.woke_ms)}` : ""}</span>
            </span>
            <Badge variant={tone(r)}>{r.error ? "failed" : (r.status ?? "…")}</Badge>
            <span class="muted kind">{kind(r.content_type)}</span>
            <span class="muted num">{r.done ? bytes(r.res_size) : ""}</span>
            <span class="muted num">{took(r.ms)}</span>
          </button>
        {:else}
          <Empty title="Nothing matches" description="No request has that." />
        {/each}
      </div>

      {#if selected != null}
        <section class="request-detail">
          {#if !detail}
            <p class="muted">Loading…</p>
          {:else}
            <div class="detail-head">
              <Badge variant={tone(detail.row)}>{detail.row.status ?? "…"} {detail.row.reason}</Badge>
              <span class="mono">{detail.row.method}</span>
              <span class="spacer"></span>
              <button class="icon-btn" title="Close" onclick={() => (selected = null)}>✕</button>
            </div>
            <p class="mono detail-url">{url(detail.row)}</p>
            {#if detail.row.error}<Callout variant="warning">{detail.row.error}</Callout>{/if}
            <p class="muted">
              {[
                detail.row.container,
                detail.row.woke_ms != null ? `it was parked: woke in ${took(detail.row.woke_ms)}` : null,
                detail.row.wait_ms != null ? `waited ${took(detail.row.wait_ms)}` : null,
                detail.row.ms != null || !detail.row.done ? `took ${took(detail.row.ms)}` : null,
                `sent ${bytes(detail.row.req_size)}, got ${bytes(detail.row.res_size)}`,
              ].filter(Boolean).join(" · ")}
            </p>
            <div class="chips">
              <Button size="sm" onclick={() => clipboard.writeText(curl(detail)).then(() => toast("Copied as curl"))}>Copy as curl</Button>
              <Button size="sm" onclick={() => replay(detail)}>Replay</Button>
              <Button size="sm" title="The request, the response and the container logs meanwhile, as Markdown (secrets in headers left out)" onclick={() => clipboard.writeText(forAi(detail, during)).then(() => toast("Copied for AI"))}>Copy for AI</Button>
              {#if detail.row.method === "GET"}<Button size="sm" variant="ghost" onclick={() => shell.openExternal(url(detail.row))}>Open</Button>{/if}
            </div>
            <Tabs
              tabs={[
                { id: "headers", label: "Headers" },
                { id: "request", label: "Request", disabled: !detail.req_body },
                { id: "response", label: "Response", disabled: !detail.res_body },
                { id: "logs", label: `Logs${during?.lines.length ? ` (${during.lines.length})` : ""}` },
              ]}
              bind:value={tab}
              label="Request"
            >
              {#snippet panel(id)}
                {#if id === "logs"}
                  {#if duringError}
                    <Callout variant="warning">{duringError}</Callout>
                  {:else if !during}
                    <p class="muted">Loading…</p>
                  {:else}
                    <p class="muted small">
                      What {during.containers.length > 1 ? `${during.containers.length} containers of its project` : during.containers[0]} wrote from {BEFORE_S} s before the request to {AFTER_S} s after its response{detail.row.done ? "" : " (so far)"}.
                    </p>
                    {#if during.lines.length}
                      <div class="log-box during">
                        {#each during.lines as l, i (i)}
                          <div class="line" class:err={l.stderr} class:outside={outside(l, detail.row)}>
                            <span class="muted mono at">{offset(l.offset)}</span>
                            {#if during.containers.length > 1}<span class="svc" class:mine={l.container === detail.row.container}>{who(l)}</span>{/if}
                            <span class="text">{l.text}</span>
                          </div>
                        {/each}
                      </div>
                      {#if during.cut}<p class="muted small">The first 1,000 lines.</p>{/if}
                    {:else}
                      <p class="muted">Nothing was written meanwhile.</p>
                    {/if}
                  {/if}
                {:else if id === "headers"}
                  <h4>Request</h4>
                  <table class="kv"><tbody>{#each detail.req_headers as [k, v], i (i)}<tr><td>{k}</td><td class="mono">{v}</td></tr>{/each}</tbody></table>
                  <h4>Response</h4>
                  {#if detail.res_headers.length}
                    <table class="kv"><tbody>{#each detail.res_headers as [k, v], i (i)}<tr><td>{k}</td><td class="mono">{v}</td></tr>{/each}</tbody></table>
                  {:else}<p class="muted">{detail.row.done ? "No response." : "No response yet."}</p>{/if}
                {:else}
                  {@const body = id === "request" ? detail.req_body : detail.res_body}
                  {#if body?.binary}
                    <p class="muted">Binary, {bytes(id === "request" ? detail.row.req_size : detail.row.res_size)}.</p>
                  {:else if body}
                    {#if body.cut}<p class="muted">The first 32 KB:</p>{/if}
                    <pre class="raw body">{pretty(body)}</pre>
                  {/if}
                {/if}
              {/snippet}
            </Tabs>
          {/if}
        </section>
      {/if}
    </div>
  {/if}
</div>
