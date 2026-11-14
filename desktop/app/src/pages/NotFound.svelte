<script>
  import { href } from "@elyra/runtime";
  let { path } = $props();
</script>

<div class="card">
  <h2>Not found</h2>
  <p class="subtitle">Nothing lives at <code>{path}</code>.</p>
  <a class="btn" href={href("/")}>Back home</a>
</div>
