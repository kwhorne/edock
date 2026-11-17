<!-- One image: what each Dockerfile line added, and its known
     vulnerabilities (Grype, from eDock's store) with an SBOM. -->
<script>
  import { channel, dialog, navigate, shell, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Empty, Input, Select, Skeleton, Switch, Tabs } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import Icon from "../lib/Icon.svelte";
  import { api } from "../bindings";
  import { engine, useContext, attempt, errorText, runImage } from "../lib/state.svelte.js";
  import { bytes, ago } from "../lib/format.js";
  import { loadIntoK8s } from "../lib/k8s.js";

  let { params, query } = $props();
  const name = $derived(decodeURIComponent(params.ref));
  const onEdock = $derived(engine.status?.context === "edock");
  // Into the cluster, while there is one running.
  let k8sReady = $state(false);
  let loading = $state(false);
  $effect(() => {
    if (onEdock) api.k8s_status().then((s) => (k8sReady = s.ready), () => (k8sReady = false));
  });
  async function toK8s() {
    loading = true;
    await loadIntoK8s(name);
    loading = false;
  }

  let tab = $state("layers");
  $effect(() => {
    if (query?.tab) tab = query.tab;
  });

  // ── Layers ──
  let layers = $state(null);
  let layersError = $state(null);
  async function loadLayers() {
    try {
      layers = await api.image_layers(name);
      layersError = null;
    } catch (e) {
      layersError = errorText(e);
    }
  }

  // ── Security ──
  let scans = $state(null);
  let current = $state(null);
  let picked = $state(null);
  let progress = $state(null);
  let severity = $state("all");
  let fixable = $state(false);
  let search = $state("");

  async function loadScans(at = null) {
    try {
      scans = await api.image_scans(name);
      const when = at ?? scans[0]?.scanned ?? null;
      picked = when;
      current = when != null ? await api.image_scan_result(name, when) : null;
    } catch (e) {
      toast(errorText(e), { variant: "error" });
    }
  }

  $effect(() => {
    if (!onEdock) return;
    name;
    loadLayers();
    loadScans();
  });

  const off = channel("scan:progress").subscribe((p) => {
    if (!p || p.name !== name) return;
    if (p.done) {
      progress = null;
      if (p.error) toast(p.error, { variant: "error", duration: 8000 });
      else loadScans();
    } else progress = p;
  });
  onDestroy(off);

  async function scan() {
    progress = { phase: "scanner", pct: 0 };
    const ok = await attempt(() => api.image_scan(name).then(() => true));
    if (!ok) progress = null;
  }

  async function saveSbom() {
    const safe = name.replace(/[^A-Za-z0-9._-]+/g, "-");
    const file = await dialog.save({ title: "Save SBOM", defaultName: `${safe}.cdx.json`, filters: [{ name: "CycloneDX JSON", extensions: ["json"] }] });
    if (file) await attempt(() => api.image_sbom_save(name, current.scanned, file), "SBOM saved");
  }

  const PHASE = { scanner: "Downloading the scanner", database: "Updating the vulnerability database", scanning: "Scanning" };
  const TONE = { Critical: "danger", High: "danger", Medium: "warning", Low: "neutral", Negligible: "neutral", Unknown: "neutral" };
  const shown = $derived.by(() => {
    if (!current) return [];
    const q = search.trim().toLowerCase();
    return current.vulns.filter(
      (v) =>
        (severity === "all" || v.severity === severity) &&
        (!fixable || v.fixed.length) &&
        (!q || v.id.toLowerCase().includes(q) || v.package.toLowerCase().includes(q)),
    );
  });
  const count = (sev) => current?.vulns.filter((v) => v.severity === sev).length ?? 0;
</script>

<div class="page detail">
  <header class="page-head">
    <button class="icon-btn" title="Back" onclick={() => navigate("/images")}><Icon name="back" /></button>
    <h1 class="mono">{name}</h1>
    {#if layers}<span class="muted">{layers.platform} · {bytes(layers.size)}</span>{/if}
    <span class="spacer"></span>
    {#if k8sReady && !name.startsWith("sha256:")}
      <Button size="sm" loading={loading} onclick={toK8s} title="Into the Kubernetes cluster, where pods use it without a registry"><Icon name="upload" size={14} /> Load into Kubernetes</Button>
    {/if}
    <Button size="sm" variant="primary" onclick={() => runImage(name)}><Icon name="play" size={14} /> Run…</Button>
  </header>

  {#if !onEdock}
    <Callout variant="info" title="Layers and security scans are eDock's own">
      This window shows <strong>{engine.status?.context}</strong>.
      <div class="callout-action"><Button size="sm" onclick={() => useContext("edock")}>Switch to eDock</Button></div>
    </Callout>
  {:else}
    <Tabs tabs={[{ id: "layers", label: "Layers" }, { id: "security", label: "Security" }]} bind:value={tab} label="Image">
      {#snippet panel(id)}
        {#if id === "layers"}
          {#if layersError}
            <Callout variant="warning" title="Couldn't read the layers">{layersError}</Callout>
          {:else if !layers}
            <Skeleton lines={6} />
          {:else}
            <table class="layers">
              <tbody>
                {#each layers.layers as l, i (i)}
                  <tr class:empty={l.empty}>
                    <td class="mono by" title={l.by}>{l.by || "—"}</td>
                    <td>{#if l.base}<Badge>base</Badge>{/if}</td>
                    <td class="size">{l.empty ? "" : bytes(l.size)}</td>
                    <td class="bar">{#if !l.empty && layers.size}<span style="width: {Math.max(1, (l.size / layers.size) * 100)}%"></span>{/if}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {:else}
          <div class="security">
            <div class="security-bar">
              {#if progress}
                <span class="muted">{PHASE[progress.phase] ?? progress.phase}{progress.pct ? ` · ${progress.pct}%` : "…"}</span>
                <Button size="sm" loading>Scanning</Button>
              {:else}
                {#if scans?.length > 1}
                  <Select value={picked} options={scans.map((s) => ({ value: s.scanned, label: `Scan ${ago(s.scanned)}` }))} onchange={(v) => loadScans(v)} />
                {/if}
                <span class="spacer"></span>
                {#if current}<Button size="sm" onclick={saveSbom}><Icon name="download" size={14} /> SBOM</Button>{/if}
                <Button size="sm" variant="primary" onclick={scan}>{current ? "Scan again" : "Scan"}</Button>
              {/if}
            </div>

            {#if scans === null}
              <Skeleton lines={4} />
            {:else if !current && !progress}
              <Empty
                title="Not scanned yet"
                description="eDock looks for known vulnerabilities in the image's packages with Grype, without starting the VM. The first scan downloads the scanner and its database (about 200 MB)."
              />
            {:else if current}
              <div class="chips">
                {#each ["Critical", "High", "Medium", "Low"] as sev (sev)}
                  <Badge variant={count(sev) ? TONE[sev] : "neutral"}>{count(sev)} {sev.toLowerCase()}</Badge>
                {/each}
                <span class="muted">· {current.vulns.filter((v) => v.fixed.length).length} fixable · {current.packages} packages · scanned {ago(current.scanned)}</span>
              </div>
              {#if current.vulns.length === 0}
                <Callout variant="success" title="No known vulnerabilities">Grype found none in {current.packages} packages.</Callout>
              {:else}
                <div class="logs-bar">
                  <Input bind:value={search} placeholder="CVE or package" aria-label="Filter vulnerabilities" />
                  <Select value={severity} options={[{ value: "all", label: "All severities" }, "Critical", "High", "Medium", "Low", "Negligible", "Unknown"]} onchange={(v) => (severity = v)} />
                  <label class="toggle"><Switch bind:checked={fixable} /> Fixable only</label>
                </div>
                <table class="vulns">
                  <thead><tr><th>Severity</th><th>Vulnerability</th><th>Package</th><th>Fixed in</th><th>From</th></tr></thead>
                  <tbody>
                    {#each shown as v (`${v.id}|${v.package}|${v.version}`)}
                      <tr>
                        <td><Badge variant={TONE[v.severity]}>{v.severity}</Badge></td>
                        <td class="mono">
                          {#if v.url}<button class="link" title={v.description} onclick={() => shell.openExternal(v.url)}>{v.id}</button>{:else}{v.id}{/if}
                        </td>
                        <td class="mono">{v.package} <span class="muted">{v.version}</span></td>
                        <td class="mono">{v.fixed.length ? v.fixed.join(", ") : "—"}</td>
                        <td class="mono by" title={v.by ?? ""}>{v.base ? "base image" : (v.by ?? "")}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              {/if}
              <p class="muted">Grype {current.scanner} · database from {current.db ? current.db.slice(0, 10) : "?"}</p>
            {/if}
          </div>
        {/if}
      {/snippet}
    </Tabs>
  {/if}
</div>
