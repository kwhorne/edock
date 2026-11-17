<!-- The VM's CPUs and memory, and what it uses now. -->
<script>
  import { confirm } from "@elyra/runtime";
  import { Badge, Button, Callout, Card, Skeleton } from "@elyra/ui";
  import { onDestroy } from "svelte";
  import { api } from "../bindings";
  import { attempt, changes, errorText } from "./state.svelte.js";
  import { bytes, percent } from "./format.js";

  let vm = $state(null);
  let error = $state(null);
  let form = $state({ cpus: 0, memory_gib: 0 });
  let busy = $state(null);

  async function load(resetForm = false) {
    try {
      vm = await api.vm_info();
      error = null;
      if (resetForm || !form.cpus) form = { cpus: vm.cpus, memory_gib: Math.round(vm.memory_mib / 1024) };
    } catch (e) {
      error = errorText(e);
    }
  }
  $effect(() => {
    changes.engine;
    load(true);
  });
  // What it uses changes all the time: every few seconds while shown.
  const timer = setInterval(() => !document.hidden && load(), 4000);
  onDestroy(() => clearInterval(timer));

  const changed = $derived(vm && (form.cpus !== vm.cpus || form.memory_gib * 1024 !== vm.memory_mib));
  const pending = $derived(vm?.awake && (vm.running_cpus !== vm.cpus || Math.abs(vm.running_memory_mib - vm.memory_mib) > 512));

  async function save() {
    busy = "save";
    const v = await attempt(() => api.vm_set_resources(Number(form.cpus), Number(form.memory_gib) * 1024), "Saved: the VM uses it from its next start");
    busy = null;
    if (v) vm = v;
  }
  async function restartVm() {
    if (!(await confirm("Restart the VM now? Running containers stop; those with a restart policy come back.", { confirmLabel: "Restart VM" }))) return;
    busy = "restart";
    await attempt(() => api.vm_restart(), "The VM restarted");
    busy = null;
    load(true);
  }
</script>

{#if error}
  <Callout variant="info" title="Resources are eDock's own">{error}</Callout>
{:else if !vm}
  <Skeleton lines={5} />
{:else}
  <Card title="Now">
    <div class="engine-card">
    <dl class="facts-grid">
      <dt>VM</dt><dd>{#if vm.awake}<Badge variant="success">awake</Badge> {vm.running_cpus} CPUs, {Math.round(vm.running_memory_mib / 1024)} GB{:else}<Badge variant="accent">asleep</Badge> it wakes when something needs it{/if}</dd>
      <dt>Containers use</dt><dd>{vm.awake ? `${bytes(vm.memory_used)} of ${bytes(vm.memory_ceiling)}` : "—"} {#if vm.awake && vm.memory_ceiling}<span class="meter {vm.memory_pressure}"><span style="width: {Math.min(100, (vm.memory_used / vm.memory_ceiling) * 100)}%"></span></span>{/if}</dd>
      {#if vm.awake}
        <dt>Memory pressure</dt>
        <dd>
          <Badge variant={vm.memory_pressure === "high" ? "danger" : vm.memory_pressure === "some" ? "warning" : "success"}>{vm.memory_pressure === "high" ? "high" : vm.memory_pressure === "some" ? "some" : "none"}</Badge>
          <span class="muted">{vm.memory_stall ? `waiting on memory ${vm.memory_stall.toFixed(1)}% of the last 10 s` : "nothing waits on memory"}</span>
          {#if vm.memory_pressure !== "none"}
            <div class="muted small">
              {#if vm.memory_ceiling && vm.memory_used / vm.memory_ceiling >= 0.85}
                The VM's memory is nearly all used: give it more below, or stop what you don't need, before the kernel stops a container.
              {:else}
                The VM has room, so a container's own memory limit (<code>-m</code>, <code>mem_limit</code>) is the likely cause: its Stats tab shows which one is at its limit. Raise that limit, or give the VM more below.
              {/if}
            </div>
          {/if}
        </dd>
      {/if}
      <dt>CPU</dt><dd>{vm.awake ? percent(vm.cpu_percent) : "—"}</dd>
      <dt>The engine</dt><dd>{bytes(vm.engine_memory)} on the Mac</dd>
      <dt>Guest agent</dt><dd>{vm.awake ? `${bytes(vm.agent_memory)} in the VM` : "—"}</dd>
      {#if vm.oom_kills}<dt>Out of memory</dt><dd><Badge variant="danger">{vm.oom_kills} process{vm.oom_kills === 1 ? "" : "es"} killed</Badge> give the VM more memory</dd>{/if}
    </dl>
    <p class="muted text">Memory the containers don't use goes back to macOS while the VM runs, and all of it when it sleeps.</p>
    </div>
  </Card>

  <Card title="Size">
    <div class="engine-card">
      {#if vm.overridden}
        <Callout variant="info" title="Set by the environment">EDOCK_CPUS or EDOCK_MEMORY_MIB fix the VM's size; changes here wait until they're gone.</Callout>
      {/if}
      <label class="slider">
        <span class="slider-head">CPUs <strong>{form.cpus}</strong> <span class="muted">of {vm.max_cpus}</span></span>
        <input type="range" min="1" max={vm.max_cpus} bind:value={form.cpus} />
      </label>
      <label class="slider">
        <span class="slider-head">Memory <strong>{form.memory_gib} GB</strong> <span class="muted">the most it may take, of {Math.round(vm.max_memory_mib / 1024)} GB</span></span>
        <input type="range" min="2" max={Math.floor(vm.max_memory_mib / 1024)} bind:value={form.memory_gib} />
      </label>
      {#if pending}
        <p class="muted">The VM still runs with its old size: restart it to use the new one.</p>
      {/if}
    </div>
    {#snippet actions()}
      {#if pending}<Button size="sm" loading={busy === "restart"} onclick={restartVm}>Restart VM</Button>{/if}
      <Button size="sm" variant="primary" disabled={!changed} loading={busy === "save"} onclick={save}>Save</Button>
    {/snippet}
  </Card>
{/if}
