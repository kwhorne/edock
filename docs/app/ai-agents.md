# AI agents (MCP)

eDock is an [MCP](https://modelcontextprotocol.io) server. An AI agent in Claude Code, Claude Desktop, Cursor or VS Code can see your containers, read their logs, check ports and image scans, and run commands in throwaway containers.

Looking is free. Anything that changes something (start, stop, pull, run, exec) is asked in the agent's app first, and nothing happens if you say no.

## Set it up

**Engine › AI agents** shows the exact command for this Mac. For Claude Code:

```sh
claude mcp add edock -- "/Applications/eDock.app/Contents/MacOS/edock-desktop" --mcp
```

For Claude Desktop, Cursor and VS Code, add this to their MCP settings (the page has a **Copy** button):

```json
{
  "mcpServers": {
    "edock": {
      "command": "/Applications/eDock.app/Contents/MacOS/edock-desktop",
      "args": ["--mcp"]
    }
  }
}
```

The agent starts eDock with `--mcp` and talks to it over stdio. When the app is open, the agent works through it, and the app shows a note when an agent changes something. When it isn't, eDock runs without a window. Either way it uses the engine the app would: eDock's own, or the one picked in the toolbar.

## What an agent can do

| Tool | | Asks first |
|---|---|---|
| `engine_status` | The engine, its version, and how many containers run | |
| `containers_list`, `container_inspect` | Containers, with their state, ports and Compose project | |
| `container_logs` | A container's recent log as text; `errors_only` keeps the lines that read like an error | |
| `container_ports` | The ports a container really listens on, published or not | |
| `domains_info` | Where containers answer (`<name>.edock.localhost`) | |
| `http_requests`, `http_request` | Requests through container domains, while [Requests](requests.md) records | |
| `http_request_logs` | What the containers (the one that answered and the rest of its Compose project) wrote while a request was handled | |
| `alerts_recent` | Containers that ran out of memory or turned unhealthy in the last hour | |
| `compose_projects` | Compose projects eDock remembers, those that are down too | |
| `compose_view` | What a Compose project is made of: each service's image or build, ports, volumes, dependencies, and its variables' names (never their values) | |
| `images_list`, `image_layers` | Images, and each Dockerfile line's size | |
| `image_run_defaults` | What Run image… suggests for an image: a name, its ports, where it keeps data | |
| `image_scans`, `image_scan_result` | Vulnerability scans | |
| `volumes_list`, `networks_list` | Volumes and networks | |
| `machines_list`, `machine_ssh` | Linux machines, and whether `ssh NAME.edock` reaches them | |
| `container_files`, `container_file_preview` | A container's folders, and the start of a file | |
| `recipes_list` | The services a recipe starts | |
| `engine_log` | eDock's engine log, or only its warnings and errors | |
| `k8s_status`, `k8s_items` | The Kubernetes cluster, its pods (and why one is stuck) and services | |
| `k8s_pod_events` | What Kubernetes said about a pod | |
| `container_action`, `project_action` | Start, stop, restart, pause, unpause, kill or remove a container or a Compose project | Yes |
| `image_pull` | Pull an image | Yes |
| `project_up` | `docker compose up -d` (with `--build`) for a Compose project, one that is down too | Yes |
| `service_up` | Make one service of a Compose project again (`up -d --no-deps --force-recreate`, with `--build` if asked), the rest left as they are | Yes |
| `compose_project_open`, `compose_project_forget` | Remember a Compose project from its folder, or forget one | Yes |
| `image_run` | A container from an image, as `docker run -d`: name, ports, Mac folders and volumes, variables, command | Yes |
| `k8s_load` | Load an image from eDock into the Kubernetes cluster | Yes |
| `volume_clone` | Copy a volume into a new one | Yes |
| `recipe_start` | Start PostgreSQL, MySQL, ElyraSQL, SQL Anywhere, Redis, RabbitMQ… for development, or Askr for a project folder | Yes |
| `snapshots_list` | Snapshots of containers and volumes | |
| `container_snapshot`, `volume_snapshot` | Take a snapshot | Yes |
| `container_snapshot_restore`, `volume_snapshot_restore` | Go back to one | Yes |
| `container_run` | Run a command once in a fresh container, then remove it | Yes |
| `container_exec` | Run a command in a running container | Yes |

`container_run` is the safe place to try something: the container has **no network** unless the agent asks for it, stops after 60 seconds (at most 10 minutes), and is removed afterwards. It touches nothing on the Mac. Output longer than 64 KB is cut.

Agents can't create or remove volumes or networks, change the engine's settings or your SSH settings, or reach Kubernetes beyond reading it and loading an image into it.

## Try it

Ask the agent things like:

- *Why does my `api` container keep restarting?*
- *Which of my containers listen on a port that isn't published?*
- *Does `node:22-alpine` have `git`?* (it runs `container_run` and asks you first)
- *Which of my images have fixable critical vulnerabilities?*

## Check it from a terminal

The [MCP Inspector](https://github.com/modelcontextprotocol/inspector) lists eDock's tools and lets you call them by hand:

```sh
npx @modelcontextprotocol/inspector "/Applications/eDock.app/Contents/MacOS/edock-desktop" --mcp
```
