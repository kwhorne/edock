<script>
  import { channel, clipboard, confirm, navigate, shell, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Checkbox, Empty, Select, Skeleton, Switch, Table, Tabs } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, useContext, attempt, errorText } from "../lib/state.svelte.js";
  import { ago } from "../lib/format.js";

  let status = $state(null);
  let items = $state(null);
  let error = $state(null);
  let progress = $state([]);
  let useAsCurrent = $state(false);
  let tab = $state("pods");
  let namespace = $state("all");
  let showSystem = $state(false);
  let podQuery = $state({});
  let svcQuery = $state({});

  async function loadStatus() {
    status = await api.k8s_status();
  }

  async function loadItems() {
    if (!status?.ready) return;
    try {
      items = await api.k8s_items();
      error = null;
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    changes.k8s;
    changes.engine;
    loadStatus().then(loadItems);
  });

  // Pods are the node's own (containerd inside it): the engine says nothing
  // when they change, so the page asks every few seconds while it's open.
  // One call at a time: a slow node doesn't pile up kubectl processes.
  let polling = false;
  const timer = setInterval(async () => {
    if (polling || document.hidden) return;
    polling = true;
    try {
      await loadStatus();
      await loadItems();
    } finally {
      polling = false;
    }
  }, 3000);
  onDestroy(() => clearInterval(timer));

  const off = channel("k8s:progress").subscribe((p) => {
    if (!p) return;
    if (p.line) progress = [...progress.slice(-30), p.line];
    if (p.done) {
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
      loadStatus().then(loadItems);
    }
  });
  onDestroy(off);

  async function cluster(action) {
    if (action === "delete" && !(await confirm("Delete the Kubernetes cluster, with everything deployed in it?", { danger: true, confirmLabel: "Delete" }))) return;
    progress = [];
    await attempt(() => api.k8s_cluster(action, useAsCurrent));
    await loadStatus();
  }

  const running = $derived(status?.state === "running");
  const SYSTEM = ["kube-system", "kube-public", "kube-node-lease"];
  const namespaces = $derived(items ? [...new Set([...items.pods, ...items.services].map((i) => i.namespace))].sort() : []);
  const visible = (i) => (namespace === "all" ? showSystem || !SYSTEM.includes(i.namespace) : i.namespace === namespace);
  const pods = $derived(items ? items.pods.filter(visible).map((p) => ({ ...p, key: `${p.namespace}/${p.name}` })) : []);
  const services = $derived(items ? items.services.filter(visible).map((s) => ({ ...s, key: `${s.namespace}/${s.name}` })) : []);

  const tone = (s) =>
    s === "Running" || s === "Completed" || s === "Succeeded"
      ? "success"
      : /BackOff|Error|Failed|OOMKilled|Err/.test(s)
        ? "danger"
        : s === "Terminating" || s === "Pending" || s === "ContainerCreating"
          ? "warning"
          : "neutral";

  const age = (ts) => (ts ? ago(Date.parse(ts) / 1000) : "—");

  async function deletePod(p) {
    const what = p.owner?.startsWith("Deployment/") ? `${p.name}? Its Deployment makes a new one.` : `${p.name}?`;
    if (!(await confirm(`Delete the pod ${what}`, { danger: true, confirmLabel: "Delete" }))) return;
    await attempt(() => api.k8s_pod_delete(p.namespace, p.name));
    loadItems();
  }

  async function restart(p) {
    const name = p.owner.slice("Deployment/".length);
    await attempt(() => api.k8s_rollout_restart(p.namespace, name), `Restarting ${name}`);
    loadItems();
  }

  // A LoadBalancer service's port is on the Mac, through the engine.
  const reachable = (s) => s.kind === "LoadBalancer";

  const kubectl = "kubectl --context edock get pods -A";

  // Forwards to the Mac: a ClusterIP service reached on 127.0.0.1.
  let forwards = $state([]);
  async function loadForwards() {
    forwards = await api.k8s_forwards();
  }
  loadForwards();
  const fwFor = (s, p) => forwards.find((f) => f.ip === s.cluster_ip && f.port === p.port);
  async function forward(s, p) {
    const f = await attempt(() => api.k8s_forward(s.cluster_ip, p.port));
    if (f) {
      await loadForwards();
      toast(`${s.name}:${p.port} is on localhost:${f.local}`, { variant: "success" });
    }
  }
  async function unforward(f) {
    await attempt(() => api.k8s_unforward(f.local));
    loadForwards();
  }
</script>

<div class="page">
  <header class="page-head">
    <h1>Kubernetes</h1>
    {#if status?.version}<span class="muted">k3s {status.version}</span>{/if}
    <span class="spacer"></span>
    {#if status && !status.on_edock}
      <!-- nothing: the callout below says what to do -->
    {:else if status?.busy}
      <Button size="sm" loading>{status.busy === "start" ? "Starting…" : status.busy === "stop" ? "Stopping…" : "Deleting…"}</Button>
    {:else if running}
      <Button size="sm" onclick={() => cluster("stop")}><Icon name="stop" size={14} /> Stop</Button>
      <Button size="sm" variant="danger" onclick={() => cluster("delete")}><Icon name="trash" size={14} /></Button>
    {:else if status && status.state !== "absent"}
      <!-- A new cluster starts from the empty page, with its kubectl choice. -->
      <Button size="sm" variant="danger" onclick={() => cluster("delete")}><Icon name="trash" size={14} /></Button>
      <Button size="sm" variant="primary" onclick={() => cluster("start")}><Icon name="play" size={14} /> Start</Button>
    {/if}
  </header>

  {#if !status}
    <Skeleton lines={4} />
  {:else if !status.on_edock}
    <Callout variant="info" title="Kubernetes runs on the eDock engine">
      This window shows <strong>{engine.status?.context}</strong>. The cluster is a k3s node on eDock's own engine.
      <div class="callout-action"><Button size="sm" onclick={() => useContext("edock")}>Switch to eDock</Button></div>
    </Callout>
  {:else if status.busy || (progress.length && !running)}
    <Card title={status.busy === "start" ? "Starting Kubernetes" : status.busy ? "Working" : "Kubernetes"}>
      <pre class="progress">{progress.join("\n") || "…"}</pre>
    </Card>
  {:else if status.state === "absent"}
    <Empty title="No cluster yet" description="A single-node Kubernetes (k3s) in one container on eDock's engine. The first start pulls k3s and takes about a minute.">
      {#snippet action()}
        <div class="start-row">
          <Checkbox class="toggle" bind:checked={useAsCurrent}>Make <code>edock</code> kubectl's current context</Checkbox>
          <Button variant="primary" onclick={() => cluster("start")}><Icon name="play" size={14} /> Start Kubernetes</Button>
        </div>
      {/snippet}
    </Empty>
  {:else if !running}
    <Empty title="Kubernetes is stopped" description="Its state is kept. Start it to carry on where it was." />
  {:else}
    <div class="k8s-facts">
      <div>
        <span class="muted">API server</span>
        <span class="mono">https://127.0.0.1:{status.port}</span>
        <Badge variant={status.ready ? "success" : "warning"}>{status.ready ? "ready" : "starting"}</Badge>
      </div>
      <div>
        <span class="muted">kubectl</span>
        <button class="copy-cmd mono" title="Copy" onclick={() => clipboard.writeText(kubectl).then(() => toast("Copied"))}>{kubectl} <Icon name="copy" size={12} /></button>
      </div>
    </div>

    {#if error}<Callout variant="warning" title="kubectl didn't answer">{error}</Callout>{/if}

    <div class="k8s-bar">
      <Select
        value={namespace}
        options={[{ value: "all", label: "All namespaces" }, ...namespaces.map((n) => ({ value: n, label: n }))]}
        onchange={(v) => (namespace = v)}
      />
      {#if namespace === "all"}<label class="toggle"><Switch bind:checked={showSystem} /> System namespaces</label>{/if}
    </div>

    <Tabs tabs={[{ id: "pods", label: `Pods (${pods.length})` }, { id: "services", label: `Services (${services.length})` }]} bind:value={tab} label="Kubernetes">
      {#snippet panel(id)}
        {#if !items}
          <Skeleton lines={5} />
        {:else if id === "pods"}
          <Table
            rows={pods}
            key="key"
            bind:query={podQuery}
            search
            sortable={["name", "namespace", "status", "restarts", "created"]}
            label="Pods"
            resizable
            persist="k8s-pods"
            onrowclick={(p) => navigate(`/kubernetes/${p.namespace}/${p.name}`)}
            columns={[
              { key: "name", label: "Name" },
              { key: "namespace", label: "Namespace", width: 130 },
              { key: "status", label: "Status", width: 150 },
              { key: "ready", label: "Ready", width: 70 },
              { key: "restarts", label: "Restarts", width: 80, align: "end" },
              { key: "created", label: "Age", width: 100 },
            ]}
          >
            {#snippet cell(row, column)}
              {#if column.key === "status"}
                <Badge variant={tone(row.status)}>{row.status}</Badge>
              {:else if column.key === "created"}
                {age(row.created)}
              {:else if column.key === "name"}
                <span class="mono">{row.name}</span>
                {#if row.owner}<div class="sub muted">{row.owner}</div>{/if}
              {:else}
                {row[column.key]}
              {/if}
            {/snippet}
            {#snippet actions(row)}
              <button class="icon-btn" title="Logs" onclick={() => navigate(`/kubernetes/${row.namespace}/${row.name}?tab=logs`)}><Icon name="logs" size={14} /></button>
              <button class="icon-btn" title="Shell" disabled={row.status !== "Running"} onclick={() => navigate(`/kubernetes/${row.namespace}/${row.name}?tab=terminal`)}><Icon name="terminal" size={14} /></button>
              {#if row.owner?.startsWith("Deployment/")}
                <button class="icon-btn" title="Restart the Deployment" onclick={() => restart(row)}><Icon name="restart" size={14} /></button>
              {/if}
              <button class="icon-btn danger" title="Delete pod" onclick={() => deletePod(row)}><Icon name="trash" size={14} /></button>
            {/snippet}
            {#snippet empty()}<span class="muted">No pods{namespace === "all" && !showSystem ? " outside the system namespaces" : ""}. <code>kubectl --context edock apply -f …</code></span>{/snippet}
          </Table>
        {:else}
          <Table
            rows={services}
            key="key"
            bind:query={svcQuery}
            search
            sortable={["name", "namespace", "kind"]}
            label="Services"
            resizable
            persist="k8s-services"
            columns={[
              { key: "name", label: "Name" },
              { key: "namespace", label: "Namespace", width: 130 },
              { key: "kind", label: "Type", width: 120 },
              { key: "cluster_ip", label: "Cluster IP", width: 130 },
              { key: "ports", label: "Ports", width: 240 },
            ]}
          >
            {#snippet cell(row, column)}
              {#if column.key === "ports"}
                {#each row.ports as p (`${p.port}/${p.protocol}`)}
                  {#if reachable(row)}
                    <button class="port" title="Open http://localhost:{p.port}" onclick={() => shell.openExternal(`http://localhost:${p.port}`)}>{p.port}<span class="muted">→{p.target}</span></button>
                  {:else if fwFor(row, p)}
                    {@const f = fwFor(row, p)}
                    <button class="port" title="Open http://localhost:{f.local}" onclick={() => shell.openExternal(`http://localhost:${f.local}`)}>localhost:{f.local}<span class="muted">→{p.port}</span></button>
                    <button class="icon-btn" title="Stop forwarding" onclick={() => unforward(f)}><Icon name="stop" size={12} /></button>
                  {:else}
                    <span class="mono port-text">{p.port}{p.node_port ? `:${p.node_port}` : ""}/{p.protocol}</span>
                    {#if p.protocol === "TCP" && row.cluster_ip && row.cluster_ip !== "None"}
                      <button class="ghost-btn" title="Reach it from the Mac" onclick={() => forward(row, p)}>Forward</button>
                    {/if}
                  {/if}
                {/each}
              {:else if column.key === "cluster_ip" || column.key === "name"}
                <span class="mono">{row[column.key]}</span>
              {:else}
                {row[column.key]}
              {/if}
            {/snippet}
          </Table>
        {/if}
      {/snippet}
    </Tabs>
  {/if}
</div>
