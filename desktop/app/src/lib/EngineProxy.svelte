<!-- The proxy for pulls, pushes and logins, and for containers if chosen. -->
<script>
  import { Badge, Button, Card, Select, Switch } from "@elyra/ui";
  import { api } from "../bindings";
  import { attempt } from "./state.svelte.js";

  let p = $state(null);
  let form = $state(null);
  let busy = $state(false);

  async function load() {
    try {
      p = await api.proxy_info();
      form = { mode: p.mode, http: p.http, https: p.https, no_proxy: p.no_proxy, containers: p.containers };
    } catch {
      p = null;
    }
  }
  load();

  async function save(e) {
    e.preventDefault();
    busy = true;
    const r = await attempt(() => api.proxy_set(form.mode, form.http.trim(), form.https.trim(), form.no_proxy.trim(), form.containers), "Proxy saved");
    busy = false;
    if (r) p = r;
  }
</script>

{#if p && form}
  <Card title="Proxy">
    <form class="engine-card" onsubmit={save}>
      <p class="muted text">For image pulls, pushes, searches and logins. VPNs need nothing here: containers reach out through macOS's own network.</p>
      <div class="port-form">
        <label>Proxy
          <Select value={form.mode} options={[{ value: "system", label: "macOS's settings" }, { value: "custom", label: "Custom" }, { value: "none", label: "None" }]} onchange={(v) => (form.mode = v)} />
        </label>
      </div>
      {#if form.mode === "custom"}
        <div class="port-form proxy-fields">
          <label>HTTP <input bind:value={form.http} placeholder="http://proxy:3128" /></label>
          <label>HTTPS <input bind:value={form.https} placeholder="same as HTTP" /></label>
          <label>No proxy for <input bind:value={form.no_proxy} placeholder="localhost,.internal" /></label>
        </div>
      {:else if form.mode === "system"}
        <p class="muted">
          Now: {#if p.effective_http || p.effective_https}<span class="mono">{p.effective_https ?? p.effective_http}</span>{:else}no proxy{/if}
          {#if p.pac}<Badge>PAC</Badge>{/if}
        </p>
      {/if}
      <label class="toggle"><Switch bind:checked={form.containers} /> Pass it to new containers (HTTP_PROXY, HTTPS_PROXY, NO_PROXY)</label>
      <div><Button type="submit" size="sm" loading={busy}>Save</Button></div>
    </form>
  </Card>
{/if}
