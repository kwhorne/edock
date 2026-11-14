<!-- A container's ports on eDock's engine: what it publishes, what it really
     listens on, and publishing or changing them while it runs. -->
<script>
  import { shell, confirm } from "@elyra/runtime";
  import { Badge, Button, Callout, Skeleton } from "@elyra/ui";
  import Icon from "./Icon.svelte";
  import { api } from "../bindings";
  import { changes, engine, attempt, domainUrl } from "./state.svelte.js";

  let { id, row = null } = $props();

  let info = $state(null);
  let error = $state(null);
  let busy = $state(false);
  let form = $state({ container_port: "", host_port: "", proto: "tcp", local: false });

  async function load() {
    try {
      info = await api.container_ports(id);
      error = null;
    } catch (e) {
      error = e?.message ?? String(e);
    }
  }

  $effect(() => {
    id;
    changes.container;
    load();
  });

  // Ports it listens on that nothing publishes: found, not declared.
  const unpublished = $derived(
    info ? info.listening.filter((p) => !info.local.includes(p) && !info.published.some((b) => b.container_port === p && b.proto === "tcp")) : [],
  );

  async function save(ports, done) {
    busy = true;
    const ok = await attempt(() => api.container_set_ports(id, ports).then(() => true), done);
    busy = false;
    if (ok) await load();
  }

  const publish = (port) =>
    save([...info.published, { container_port: port, host_port: port, proto: "tcp", host_ip: "" }], `Published ${port}`);

  async function remove(b) {
    if (!(await confirm(`Stop publishing ${b.container_port}/${b.proto} on ${b.host_port}?`, { confirmLabel: "Remove" }))) return;
    save(info.published.filter((x) => x !== b), "Port removed");
  }

  function add(e) {
    e.preventDefault();
    const cp = Number(form.container_port);
    const hp = form.host_port === "" ? cp : Number(form.host_port);
    if (!Number.isInteger(cp) || cp < 1 || cp > 65535 || !Number.isInteger(hp) || hp < 0 || hp > 65535) return;
    save(
      [...info.published, { container_port: cp, host_port: hp, proto: form.proto, host_ip: form.local ? "127.0.0.1" : "" }],
      `Published ${cp}`,
    );
    form = { container_port: "", host_port: "", proto: "tcp", local: false };
  }

  const url = (b) => `http://localhost:${b.host_port}`;
</script>

<div class="ports-panel">
  {#if engine.status?.context !== "edock"}
    <Callout variant="info" title="Ports are changed on eDock's engine">
      Publishing a port on a running container, and finding what it listens on, are eDock's own. This window shows
      {engine.status?.context}.
    </Callout>
  {:else if error}
    <Callout variant="warning" title="Couldn't read the ports">{error}</Callout>
  {:else if !info}
    <Skeleton lines={4} />
  {:else}
    {#if row && domainUrl(row)}
      <section>
        <h3>Address</h3>
        <button class="domain" onclick={() => shell.openExternal(domainUrl(row))}>{domainUrl(row)}</button>
        <p class="muted">eDock's router sends this name to the container's web port, published or not.</p>
      </section>
    {/if}

    {#if unpublished.length}
      <Callout variant="info" title="Listening, but not published">
        The container listens on {unpublished.length === 1 ? "a port" : "ports"} nothing publishes. Publish one to reach it
        from the Mac.
        <div class="chips callout-action">
          {#each unpublished as p (p)}
            <Button size="sm" disabled={busy} onclick={() => publish(p)}><Icon name="plus" size={13} /> Publish {p}</Button>
          {/each}
        </div>
      </Callout>
    {/if}

    <section>
      <h3>Published</h3>
      {#if info.published.length}
        <table>
          <thead><tr><th>On the Mac</th><th>In the container</th><th></th></tr></thead>
          <tbody>
            {#each info.published as b (`${b.host_ip}:${b.host_port}:${b.container_port}/${b.proto}`)}
              <tr>
                <td class="mono">
                  {#if b.proto === "tcp"}
                    <button class="port" onclick={() => shell.openExternal(url(b))}>{b.host_ip || "*"}:{b.host_port}</button>
                  {:else}
                    {b.host_ip || "*"}:{b.host_port}
                  {/if}
                  {#if b.host_ip === "127.0.0.1"}<Badge>this Mac only</Badge>{/if}
                </td>
                <td class="mono">{b.container_port}/{b.proto} {#if info.listening.includes(b.container_port)}<Badge variant="success">listening</Badge>{/if}</td>
                <td><button class="icon-btn danger" title="Remove" disabled={busy} onclick={() => remove(b)}><Icon name="trash" size={14} /></button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="muted">Nothing published.</p>
      {/if}
    </section>

    <section>
      <h3>Publish a port</h3>
      <form class="port-form" onsubmit={add}>
        <label>Container port <input bind:value={form.container_port} inputmode="numeric" placeholder="80" required /></label>
        <label>Mac port <input bind:value={form.host_port} inputmode="numeric" placeholder="same" /></label>
        <label>Protocol
          <select bind:value={form.proto}><option value="tcp">TCP</option><option value="udp">UDP</option></select>
        </label>
        <label class="toggle"><input type="checkbox" bind:checked={form.local} style="width:auto" /> This Mac only</label>
        <Button type="submit" size="sm" variant="primary" loading={busy}>Publish</Button>
      </form>
      <p class="muted">The container keeps running. A Mac port of 0 lets eDock pick a free one.</p>
    </section>

    {#if info.local.length}
      <p class="muted">Also listening on its own loopback only, out of reach from outside: {info.local.join(", ")}.</p>
    {/if}
  {/if}
</div>
