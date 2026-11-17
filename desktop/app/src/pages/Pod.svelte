<!-- One pod: why it's stuck when it is, its containers and events, and a
     container's log and shell. -->
<script>
  import { confirm, navigate } from "@elyra/runtime";
  import { Badge, Button, Callout, Select, Skeleton, Tabs } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import Logs from "../lib/Logs.svelte";
  import Terminal from "../lib/Terminal.svelte";
  import { api } from "../bindings";
  import { attempt, errorText } from "../lib/state.svelte.js";
  import { localImages, loadIntoK8s, shortRef } from "../lib/k8s.js";
  import { ago } from "../lib/format.js";

  let { params, query } = $props();

  let pod = $state(null);
  let gone = $state(false);
  let error = $state(null);
  let tab = $state("logs");
  let container = $state(null);
  let events = $state(null);
  let local = $state(new Set());
  let loading = $state(false);

  $effect(() => {
    if (query?.tab) tab = query.tab;
  });
  // A stuck pod opens on what Kubernetes said about it.
  let chose = false;
  $effect(() => {
    if (!chose && pod) {
      chose = true;
      if (pod.why && !query?.tab) tab = "events";
    }
  });

  async function load() {
    try {
      const items = await api.k8s_items();
      const p = items.pods.find((x) => x.namespace === params.namespace && x.name === params.pod);
      gone = !p;
      if (p) pod = p;
      if (p && (!container || !p.containers.some((c) => c.name === container))) container = p.containers[0]?.name ?? null;
      error = null;
      if (p?.pull_failed && !local.size) local = await localImages();
      if (p && (tab === "events" || events === null)) events = await api.k8s_pod_events(params.namespace, params.pod).catch(() => events);
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    params.namespace;
    params.pod;
    load();
  });
  const timer = setInterval(() => !document.hidden && load(), 4000);
  onDestroy(() => clearInterval(timer));

  const current = $derived(pod?.containers.find((c) => c.name === container));
  const loadable = $derived(pod?.pull_failed && local.has(shortRef(pod.pull_failed)));

  async function loadImage() {
    loading = true;
    const ok = await loadIntoK8s(pod.pull_failed);
    loading = false;
    // Kubernetes tries the pull again on its own, backing off up to five
    // minutes; a new pod tries at once.
    if (ok && pod.owner && (await confirm(`Replace this pod now? ${pod.owner} makes a new one, which finds the image at once; this one would try again by itself within a few minutes.`, { confirmLabel: "Replace pod" }))) {
      if (await attempt(() => api.k8s_pod_delete(pod.namespace, pod.name).then(() => true))) navigate("/kubernetes");
    }
  }
  const age = (t) => (t ? ago(Date.parse(t) / 1000) : "");
  const tone = (s) => (s === "Running" ? "success" : /BackOff|Error|Failed|Err/.test(s) ? "danger" : "warning");

  async function remove() {
    if (!(await confirm(`Delete the pod ${pod.name}?`, { danger: true, confirmLabel: "Delete" }))) return;
    await attempt(() => api.k8s_pod_delete(pod.namespace, pod.name));
    navigate("/kubernetes");
  }
</script>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/kubernetes")}><Icon name="back" /></button>
    <h1 class="mono">{params.pod}</h1>
    <span class="muted">{params.namespace}</span>
    {#if pod}<Badge variant={tone(pod.status)}>{pod.status}</Badge>{/if}
    <span class="spacer"></span>
    {#if pod}<Button size="sm" variant="danger" onclick={remove}><Icon name="trash" size={14} /> Delete</Button>{/if}
  </header>

  {#if error}
    <Callout variant="warning" title="kubectl didn't answer">{error}</Callout>
  {:else if gone && !pod}
    <Callout variant="info" title="This pod is gone">It was deleted, or replaced by a new one. <a href="#/kubernetes">Back to the pods</a></Callout>
  {:else if !pod}
    <Skeleton lines={6} />
  {:else}
    {#if gone}<Callout variant="info" title="This pod is gone">Showing it as it was. <a href="#/kubernetes">Back to the pods</a></Callout>{/if}
    {#if pod.why && !gone}
      <Callout variant="warning" title="Why it's stuck">
        {pod.why}
        {#if loadable}
          <div class="callout-action">
            <Button size="sm" loading={loading} onclick={loadImage}><Icon name="upload" size={14} /> Load {pod.pull_failed} from eDock</Button>
            <span class="muted small">It's on eDock, not in the cluster: <code>edock k8s load {pod.pull_failed}</code> does the same.</span>
          </div>
        {:else if pod.pull_failed}
          <div class="muted small">Built on eDock? <code>docker build -t {pod.pull_failed} .</code>, then load it here, or <code>edock k8s load {pod.pull_failed}</code>.</div>
        {/if}
      </Callout>
    {/if}
    <div class="facts">
      <div><span class="muted">Ready</span>{pod.ready}</div>
      <div><span class="muted">Restarts</span>{pod.restarts}</div>
      <div><span class="muted">Pod IP</span><span class="mono">{pod.ip || "—"}</span></div>
      <div><span class="muted">Age</span>{pod.created ? ago(Date.parse(pod.created) / 1000) : "—"}</div>
      {#if pod.owner}<div><span class="muted">Owner</span>{pod.owner}</div>{/if}
      {#if pod.containers.length > 1}
        <div>
          <span class="muted">Container</span>
          <Select value={container} options={pod.containers.map((c) => c.name)} onchange={(v) => (container = v)} />
        </div>
      {:else if current}
        <div><span class="muted">Image</span>{current.image}</div>
      {/if}
    </div>

    <Tabs
      tabs={[
        { id: "logs", label: "Logs" },
        { id: "terminal", label: "Shell", disabled: current?.state !== "running" },
        { id: "containers", label: `Containers (${pod.containers.length})` },
        { id: "events", label: `Events${events?.length ? ` (${events.length})` : ""}` },
      ]}
      bind:value={tab}
      label="Pod"
    >
      {#snippet panel(id)}
        {#if id === "logs" && container}
          {#key container}
            <Logs start={(tail, ts) => api.k8s_logs_follow(pod.namespace, pod.name, container, tail, ts)} />
          {/key}
        {:else if id === "terminal" && container}
          {#key container}
            <Terminal start={(cols, rows) => api.k8s_exec_start(pod.namespace, pod.name, container, cols, rows)} />
          {/key}
        {:else if id === "containers"}
          <table class="kv">
            <tbody>
              {#each pod.containers as c (c.name)}
                <tr>
                  <td class="mono">{c.name}</td>
                  <td>
                    <span class="mono">{c.image}</span><br />
                    <Badge variant={c.ready ? "success" : c.state === "running" ? "warning" : "neutral"}>{c.state}{c.reason ? `: ${c.reason}` : ""}</Badge>
                    <span class="muted">{c.restarts} restart{c.restarts === 1 ? "" : "s"}{c.last ? ` · last ended: ${c.last}` : ""}</span>
                    {#if c.message}<div class="muted small">{c.message}</div>{/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else if id === "events"}
          {#if events === null}
            <Skeleton lines={3} />
          {:else if !events.length}
            <p class="muted">No events: Kubernetes keeps them for an hour.</p>
          {:else}
            <table class="kv events">
              <tbody>
                {#each events as e, i (i)}
                  <tr>
                    <td><Badge variant={e.kind === "Warning" ? "warning" : "neutral"}>{e.reason}</Badge></td>
                    <td class="msg">{e.message}</td>
                    <td class="muted nowrap">{e.count > 1 ? `${e.count}× · ` : ""}{age(e.last)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      {/snippet}
    </Tabs>
  {/if}
</div>
