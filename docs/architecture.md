# Architecture

eDock has two parts:

- **The engine** (`engine/`): the `edock` daemon, a Linux VM, and the container runtime inside it. It serves the Docker Engine API.
- **The desktop app** (`desktop/`): the window and the menu bar item, built on the [Elyra Framework](https://github.com/kwhorne/elyra-framework). It's one Docker API client among many.

```
docker CLI ─┐
compose ────┤
buildx ─────┼─ ~/.edock/run/docker.sock ─► edock daemon ── vsock ──► guest agent (PID 1)
eDock app ──┘     Docker Engine API          owns the VM            clone3 → container
```

## The engine

The engine is a fork of [Kiri](https://github.com/libracoder/kiri). See [Licensing](licensing.md).

### The daemon (`crates/edock`)

The daemon serves the Docker Engine API on a unix socket and owns the VM. It handles:

- pulling and pushing images (on the Mac, through `crates/edock-image`);
- port forwarding;
- the `*.edock.localhost` and `*.edock.local` routers with their TLS;
- DNS for container names;
- the Finder mount;
- the BuildKit builder;
- the Kubernetes node.

It boots the VM on the first request that needs it, and powers it off after 60 seconds idle. The daemon itself stays at about 15 MB. A memory balloon gives memory the containers don't use back to macOS while the VM runs.

### The VM (`crates/edock-vmm`, `guest/kernel`)

- **The VM:** Apple's Virtualization.framework, driven from Rust through objc2. Starting it needs the `com.apple.security.virtualization` entitlement.
- **The kernel:** a custom 10 MB Linux kernel built from `allnoconfig`, with a few patches:
  - virtio-pci resets wait microseconds instead of a scheduler tick;
  - big TCP goes to the host as GSO;
  - new FUSE files are owner-readable;
  - an upstream GRO fix keeps forwarded packets segmentable;
  - `binfmt_elf` passes HWCAP3 only when it's set.
- **Rosetta** is shared in over virtiofs and registered with `binfmt_misc`, so amd64 binaries run.
- **Mac folders** are shared with virtiofs for bind mounts.

### The guest agent (`crates/edock-init`)

A static Rust binary of about 900 KB that is the VM's **PID 1**. One `poll()` loop:

- serves the host over vsock;
- reaps children;
- forwards container output;
- relays ports;
- answers DNS and runs DHCP.

Big jobs such as `docker cp` run in forked children, so they never stall the loop.

A container starts with **one `clone3` call** straight into new namespaces and its cgroup (`CLONE_INTO_CGROUP`). Then:

- overlayfs over the image's layers, unpacked on an ext4 data disk;
- `pivot_root`;
- capabilities and a seccomp filter;
- networking, set up with hand-written rtnetlink and nf_tables messages.

There is **no runc, no containerd shim and no dockerd**. That is where eDock's start times come from.

### The wire protocol (`crates/edock-proto`)

Host and guest talk over vsock with a `no_std` protocol that allocates nothing per message.

### Kubernetes

`edock k8s start` runs k3s in one privileged container, `edock-k8s`:

- its state is in the `edock-k8s` volume;
- its API server is published on `127.0.0.1:6443` by default (`edock k8s set --port`, `--lan`);
- the `edock` kubectl context goes in kubectl's config.

Pods run in k3s's own containerd inside that container. The daemon publishes LoadBalancer services on the Mac.

## The desktop app

The app is an Elyra app:

- **The backend:** Rust, with commands called over MessagePack IPC.
- **The frontend:** Svelte 5 and `@elyra/ui`.
- **The window:** the system's WebKit.

### Backend (`desktop/src`)

| Module | What |
|---|---|
| `engine.rs` | The current engine: a `bollard` Docker client on the chosen context's socket, behind a lock so switching context swaps it for every command. It also reads the contexts from `~/.docker/contexts`, and keeps the app's choice in `~/.edock/desktop.json`. |
| `resources.rs` | Commands for containers, Compose projects, images, volumes and networks: lists, actions, inspect, pull and prune. Containers group by the `com.docker.compose.*` labels. |
| `streams.rs` | What streams to the window: logs, stats, terminals (exec with a TTY, input, resize), and the engine's event stream. |
| `k8s.rs` | Kubernetes (and forwarding a service to the Mac): `edock k8s start/stop/delete` as a child process with its output as progress, and `kubectl` in the node container for pods, services, logs, shells, deletes and restarts. |
| `edock.rs` | eDock's own API for containers: domains and HTTPS trust, the ports a container listens on, publishing ports and changing the environment while it runs |
| `machines.rs` | Linux machines: listing, `edock machine create` with progress, start/stop/delete, a login shell as the Mac user, and `ssh NAME.edock` on or off (`edock machine ssh`) |
| `images.rs` | An image's layers, Grype scans (streamed progress), their findings and SBOM |
| `builds.rs` | The build history, through `edock builds` |
| `vm.rs` | The VM's size and use, the BuildKit builder, importing from other engines, the diagnostics report |
| `daemon.rs` | eDock's engine: finding the `edock` binary, running `edock daemon` when none runs, and Start and Stop through the engine's own `POST /edock/wake` and `POST /edock/stop`. It also reads the engine's state (`EdockStopped`, `EdockVmAwake` in `/info`), with a small HTTP client, since a Docker client library drops those fields. |
| `compose.rs` | Compose projects as a whole: one log for all their services, their folder in Finder, Terminal or an editor, **Up** and **Rebuild & up**, and the projects remembered (seen in events and lists, or opened from a folder) so one that is down still shows |
| `run.rs` | **Run image…**: a container from an image with ports, Mac folders and volumes, variables and a command, as `docker run -d` makes one |
| `files.rs` | A container's Files tab: folders (from the engine on eDock, `find` and `stat` elsewhere), preview, save, upload and remove |
| `volumes.rs` | A volume's files through a helper container, clone, export and import |
| `snapshots.rs` | Snapshots of containers (`docker commit`) and volumes (`.tar.gz` on the Mac), and restoring them |
| `recipes.rs` | **New service**: the recipes and starting one |
| `inspector.rs` | The HTTP inspector's requests, from the engine's `/edock/http` |
| `alerts.rs` | macOS notifications when a container runs out of memory or turns unhealthy |
| `bench.rs` | The engine's log and the speed test |
| `usb.rs` | USB devices for containers, through `edock usb` |
| `agent.rs` | Commands made for AI agents over MCP: a log as text, a command in a throwaway container or a running one |
| `main.rs` | The app: its window, the menu bar item and its popover, and a provider that watches the engine and keeps the menu bar title current. |

### Live updates

The app never polls the Docker API for its lists:

1. A background task follows the engine's `/events` stream.
2. It sends each change on the `docker:changed` channel as `{kind, action}`. Bursts are coalesced to at most one every 150 ms per kind, and noise such as exec and health events is skipped.
3. Each page reloads only when its kind changes.

When the engine goes away, the task tries again every 2 seconds, and says `engine up` when it's back. Every list then reloads. A context switch does the same.

**Kubernetes pods are the exception.** They live in containerd inside the node, so the engine sees no events for them. The Kubernetes page asks for the list every 3 seconds while it's open, one request at a time.

### Streams

Logs, stats and terminals are **streams**. A command opens one and returns its id, and the data arrives on a channel of its own (`logs:<id>`, `stats:<id>`, `exec:<id>`). `stream_stop` ends it, and so does leaving the page.

- **Logs** are split into lines and sent in batches every 60 ms, at most 2,000 lines per batch. A line that arrives in pieces is put back together.
- **Terminals** send their output every 16 ms, or at once past 32 KB. A UTF-8 character split across two reads is held back until it's whole.

Container logs and `kubectl logs -f` share the same code, and so do a container's shell and `kubectl exec -it` in a pod. The Svelte `Logs` and `Terminal` components take the function that opens their stream.

### Updates

The app uses Elyra's updater (`UpdaterConfig` in `main.rs`).

1. **The check.** At launch, it fetches `https://github.com/kwhorne/edock/releases/latest/download/latest.json`. GitHub sends `latest/download/…` to the newest release's file.
2. **The offer.** When the manifest names a newer version, the runtime's toast offers it with its notes.
3. **Installing.** The toast's **Install** downloads `eDock-<version>-arm64.zip` and checks it three ways:
   - its SHA-256;
   - an **ed25519 signature** made with eDock's update key, whose public half is `UPDATE_KEY` in `main.rs`;
   - the bundle inside must be signed with **eDock's Developer ID** (team `52AT44QFCM`, bundle id `app.edock.app`), intact.
4. **Swapping.** The new app is moved into place beside the running one, and **Restart** relaunches into it.

An update that fails any check is never installed. The engine process that was running keeps the old version until **Restart engine** (`daemon_restart`). That command finds the engine's process through the socket (`LOCAL_PEERPID`), so it's the right one however it was started.

### Frontend (`desktop/app/src`)

| Path | What |
|---|---|
| `App.svelte` | The window: toolbar with the context picker, sidebar, the ⌘K commands. Also the menu bar menu's and popover's messages. |
| `Router.svelte`, `routes.js` | The pages and their paths |
| `pages/` | `Containers`, `Container`, `Images`, `Image`, `Volumes`, `Volume`, `Networks`, `Snapshots`, `Requests`, `Builds`, `Build`, `Machines`, `Machine`, `Kubernetes`, `Pod`, `Engine`, `Welcome`, `Quick` (the popover) |
| `lib/state.svelte.js` | The engine's status, and a counter per kind of change that pages watch |
| `lib/Logs.svelte`, `lib/Terminal.svelte`, `lib/Stats.svelte` | The streaming views (xterm.js for the terminal) |
| `bindings.ts` | Generated by `rata codegen`: typed `api.*` for every command |
