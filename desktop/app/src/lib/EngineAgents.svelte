<!-- AI agents: eDock as an MCP server for Claude, Cursor and VS Code. -->
<script>
  import { clipboard, toast } from "@elyra/runtime";
  import { Card } from "@elyra/ui";
  import { api } from "../bindings";

  let bin = $state("");
  api.mcp_command().then((b) => (bin = b));
  const claude = $derived(`claude mcp add edock -- "${bin}" --mcp`);
  const json = $derived(JSON.stringify({ mcpServers: { edock: { command: bin, args: ["--mcp"] } } }, null, 2));
  const copy = (t) => clipboard.writeText(t).then(() => toast("Copied"));
</script>

<Card title="AI agents">
  <div class="engine-card">
    <p class="muted text">
      eDock is an MCP server: an agent in Claude Code, Claude Desktop, Cursor or VS Code can see your containers, read
      their logs, check ports and scans, and run things in throwaway containers. Looking is free; anything that changes
      something (start, stop, pull, run, exec) is asked in the agent's app first.
    </p>
    <p class="muted">Claude Code:</p>
    <button class="copy-cmd mono" title="Copy" onclick={() => copy(claude)}>{claude}</button>
    <p class="muted">Claude Desktop, Cursor, VS Code (their MCP settings):</p>
    <pre class="progress agent-json">{json}</pre>
    <div><button class="ghost-btn" onclick={() => copy(json)}>Copy</button></div>
  </div>
</Card>
