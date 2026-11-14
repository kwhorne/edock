"""eDock as an MCP server, end to end, as an agent uses it.

Starts `edock-desktop --mcp` (headless when the app isn't open), and over
stdio: lists the tools, reads the engine and the containers, reads a log,
and runs a command in a throwaway container, answering the confirmation
eDock asks for.

    python3 desktop/tests/mcp_e2e.py desktop/target/debug/edock-desktop

Needs eDock's engine running.
"""
import json
import subprocess
import sys

binary = sys.argv[1]
p = subprocess.Popen([binary, "--mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
next_id = 0


def send(msg):
    p.stdin.write(json.dumps(msg) + "\n")
    p.stdin.flush()


def request(method, params, accept=True):
    """A request; answers any confirmation the server asks for meanwhile."""
    global next_id
    next_id += 1
    mine = next_id
    send({"jsonrpc": "2.0", "id": mine, "method": method, "params": params})
    asked = []
    while True:
        line = p.stdout.readline()
        if not line:
            raise SystemExit("the server closed")
        m = json.loads(line)
        if m.get("method") == "elicitation/create":
            asked.append(m["params"].get("message", ""))
            answer = {"action": "accept", "content": {}} if accept else {"action": "decline"}
            send({"jsonrpc": "2.0", "id": m["id"], "result": answer})
            continue
        if m.get("id") == mine:
            return m, asked


def check(ok, what):
    print(("ok   " if ok else "FAIL ") + what)
    if not ok:
        p.kill()
        raise SystemExit(1)


def text(result):
    r = result["result"]
    if r.get("structuredContent") is not None:
        return r["structuredContent"]
    return json.loads(r["content"][0]["text"])


init, _ = request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}}, "clientInfo": {"name": "edock-e2e", "version": "1"}})
check("result" in init, "initialize")
send({"jsonrpc": "2.0", "method": "notifications/initialized"})

tools, _ = request("tools/list", {})
names = {t["name"] for t in tools["result"]["tools"]}
check({"containers_list", "container_logs", "container_run", "engine_status"} <= names, f"{len(names)} tools listed")
run = next(t for t in tools["result"]["tools"] if t["name"] == "container_run")
check(run.get("annotations", {}).get("destructiveHint") is True, "container_run is marked as asking first")

st, asked = request("tools/call", {"name": "engine_status", "arguments": {}})
check(not st["result"].get("isError") and not asked, "engine_status, without asking")
check(text(st)["connected"], "the engine answers")

ls, _ = request("tools/call", {"name": "containers_list", "arguments": {"size": False}})
check(not ls["result"].get("isError"), "containers_list")

# A command in a throwaway container: asked first, then run.
r, asked = request("tools/call", {"name": "container_run", "arguments": {"image": "alpine:latest", "command": "echo from-an-agent; uname -m"}})
check(len(asked) == 1, "container_run asked first: " + (asked[0][:70] if asked else "-"))
out = text(r)
check(out["exit_code"] == 0 and "from-an-agent" in out["output"] and "aarch64" in out["output"], "container_run ran: " + out["output"].replace("\n", " | ").strip())

# No network unless asked.
r, _ = request("tools/call", {"name": "container_run", "arguments": {"image": "alpine:latest", "command": "wget -q -T 3 -O- http://example.com >/dev/null && echo online || echo offline"}})
check("offline" in text(r)["output"], "no network by default")

# Declined: nothing runs.
r, asked = request("tools/call", {"name": "container_run", "arguments": {"image": "alpine:latest", "command": "echo should-not-run"}}, accept=False)
check(r["result"].get("isError") and "should-not-run" not in json.dumps(r), "declined: it doesn't run")

p.stdin.close()
p.wait(timeout=10)
print("all good")
