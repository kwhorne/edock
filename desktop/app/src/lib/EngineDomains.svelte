<!-- Container domains: their ports, and trusting their HTTPS. -->
<script>
  import { toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card } from "@elyra/ui";
  import { api } from "../bindings";
  import { engine, attempt } from "./state.svelte.js";

  let busy = $state(null);
  let portForm = $state({ http: "", https: "" });
  $effect(() => {
    if (engine.domains?.available) portForm = { http: String(engine.domains.http_port), https: String(engine.domains.https_port) };
  });
  const httpSuffix = $derived(engine.domains && engine.domains.http_port !== 80 ? `:${engine.domains.http_port}` : "");

  async function savePorts(e) {
    e.preventDefault();
    const http = Number(portForm.http), https = Number(portForm.https);
    if (!(http > 0 && http < 65536 && https > 0 && https < 65536)) return toast("Ports are 1 to 65535", { variant: "error" });
    busy = "ports";
    const d = await attempt(() => api.domains_set_ports(http, https), "Container domains moved");
    busy = null;
    if (d) engine.domains = d;
  }
  async function trust(yes) {
    busy = "tls";
    await attempt(() => api.tls_trust(yes), yes ? "HTTPS trusted" : "HTTPS no longer trusted");
    busy = null;
    engine.domains = await api.domains_info();
  }
</script>

{#if !engine.domains?.available}
  <Callout variant="info" title="Container domains are eDock's own">Switch to eDock's engine to set them up.</Callout>
{:else}
  <Card title="Container domains">
    <div class="engine-card">
      <p class="text muted">
        Running containers answer at <span class="mono">http://&lt;name&gt;.{engine.domains.suffix}{httpSuffix}</span>,
        and Compose services at <span class="mono">&lt;service&gt;.&lt;project&gt;.{engine.domains.suffix}</span>, with no
        port to publish.
      </p>
      {#if !engine.domains.http_on}
        <Callout variant="warning" title="Port {engine.domains.http_port} is taken">
          Another server has it, or a container publishes it, so container domains are off. Give them ports of their own below.
        </Callout>
      {/if}
      <form class="port-form" onsubmit={savePorts}>
        <label>HTTP port <input bind:value={portForm.http} inputmode="numeric" /></label>
        <label>HTTPS port <input bind:value={portForm.https} inputmode="numeric" /></label>
        <Button type="submit" size="sm" loading={busy === "ports"}>Save ports</Button>
      </form>
    </div>
  </Card>
  <Card title="HTTPS">
    <div class="engine-card">
      <p>
        {#if engine.domains.trusted}
          <Badge variant="success">trusted</Badge>
          <span class="muted">Browsers show a padlock for eDock's names.</span>
        {:else}
          <Badge>not trusted</Badge>
          <span class="muted">Browsers warn about eDock's names over HTTPS.</span>
        {/if}
      </p>
      <p class="text muted">
        Trusting eDock's certificate authority asks for your password, in macOS's own prompt. Its certificates can only
        vouch for <span class="mono">*.edock.local</span> and <span class="mono">*.edock.localhost</span>, never another site.
      </p>
    </div>
    {#snippet actions()}
      {#if engine.domains.trusted}
        <Button size="sm" loading={busy === "tls"} onclick={() => trust(false)}>Stop trusting</Button>
      {:else}
        <Button size="sm" variant="primary" loading={busy === "tls"} onclick={() => trust(true)}>Trust HTTPS</Button>
      {/if}
    {/snippet}
  </Card>
{/if}
