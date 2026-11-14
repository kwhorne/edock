<!-- USB devices for containers (beta): handed to eDock's VM, and back. -->
<script>
  import { clipboard, confirm, toast } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Empty, Skeleton } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import { attempt } from "./state.svelte.js";

  let list = $state(null);
  let error = $state(null);
  let busy = $state({});

  async function load() {
    try {
      list = await api.usb_list();
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }
  load();
  // Devices come and go with the cable: look again every few seconds.
  const timer = setInterval(() => !document.hidden && load(), 3000);
  onDestroy(() => clearInterval(timer));

  async function attach(d) {
    if (!(await confirm(`Hand ${d.name} to eDock? It's gone from the Mac until you give it back. The first time, macOS asks for an administrator's password to install eDock's USB helper.`, { confirmLabel: "Hand to eDock" }))) return;
    busy[d.key] = true;
    await attempt(() => api.usb_attach(d.key), `${d.name} is in eDock`);
    busy[d.key] = false;
    load();
  }
  async function detach(d) {
    busy[d.key] = true;
    await attempt(() => api.usb_detach(d.key), `${d.name} is back on the Mac`);
    busy[d.key] = false;
    load();
  }
  const flag = (d) => d.paths.find((p) => p.path.startsWith("/dev/tty"))?.path ?? d.paths[0]?.path ?? "/dev/bus/usb";
</script>

<Callout variant="info" title="Beta">
  A device handed to eDock leaves the Mac until it's given back. Containers reach it with
  <code>--device</code>. One that resets or comes back after sleep is handed over again by itself.
</Callout>

{#if error}
  <Callout variant="warning" title="Couldn't list USB devices">{error}</Callout>
{:else if !list}
  <Skeleton lines={3} />
{:else if list.devices.length === 0}
  <Empty title="No USB devices" description="Plug one in: serial adapters, Zigbee and Z-Wave sticks, development boards, HID devices." />
{:else}
  {#each list.devices as d (d.key)}
    <Card title={d.name || d.id}>
      <div class="engine-card">
        <p>
          {#if d.state === "edock"}<Badge variant="success">in eDock</Badge>{:else}<Badge>on the Mac</Badge>{/if}
          {#if d.kind}<Badge>{d.kind}</Badge>{/if}
          <span class="muted mono">{d.id} {d.speed} {d.maker}</span>
        </p>
        {#if d.state === "edock" && d.paths.length}
          <p class="muted">In the VM: {#each d.paths as p (p.path)}<span class="mono">{p.path}</span> <span class="muted">({p.what})</span> {/each}</p>
          <p>
            <button class="copy-cmd mono" title="Copy" onclick={() => clipboard.writeText(`--device ${flag(d)}`).then(() => toast("Copied"))}>docker run --device {flag(d)} …</button>
          </p>
        {/if}
      </div>
      {#snippet actions()}
        {#if d.state === "edock"}
          <Button size="sm" loading={busy[d.key]} onclick={() => detach(d)}>Give back to the Mac</Button>
        {:else}
          <Button size="sm" variant="primary" loading={busy[d.key]} onclick={() => attach(d)}>Hand to eDock</Button>
        {/if}
      {/snippet}
    </Card>
  {/each}
{/if}
