<!--
  Renders the page for the current route. `routes` maps patterns
  ("/customers/:id") to loaders (`() => import("./pages/Customer.svelte")`),
  so each page is its own chunk. The page receives `params` and `query`.
-->
<script>
  import { route, resolveRoute } from "@elyra/runtime";
  import NotFound from "./pages/NotFound.svelte";

  let { routes } = $props();

  const match = $derived(resolveRoute(routes, $route.path));
</script>

{#if match}
  {#await match.value() then page}
    <page.default params={match.params} query={$route.query} />
  {:catch error}
    <p class="result">Could not load this page: {error.message}</p>
  {/await}
{:else}
  <NotFound path={$route.path} />
{/if}
