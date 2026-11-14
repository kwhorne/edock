# eDock documentation

eDock is a small, fast container runtime for the Mac. It runs containers in one Linux VM with its own engine, and that engine serves the Docker Engine API. So `docker`, Docker Compose, buildx and every tool built on them work unchanged. A desktop app and a menu bar item manage it all, including a single-node Kubernetes cluster.

eDock runs on **Apple silicon Macs with macOS 14 or later**.

## Using eDock

| Guide | What it covers |
|---|---|
| [Getting started](getting-started.md) | Install, first launch, starting the engine, your first container |
| [Using docker with eDock](docker.md) | The `edock` context, `edock setup`, Compose, builds, container domains, `/var/run/docker.sock` |
| [Containers](app/containers.md) | The container list, Compose projects, logs, stats, the terminal, inspect |
| [Images, volumes and networks](app/images-volumes-networks.md) | Pulling, pruning, creating and removing |
| [Kubernetes](app/kubernetes.md) | The k3s cluster: starting it, pods, services, pod logs and shells |
| [Engine and contexts](app/engine-and-contexts.md) | Starting and stopping the engine, switching to OrbStack, Docker Desktop and others |
| [The menu bar](app/menu-bar.md) | The menu bar item and its popover |
| [Updates](app/updates.md) | How eDock finds, checks and installs a new version |

## Reference

| Page | What it covers |
|---|---|
| [`edock` CLI](cli.md) | Every `edock` command |
| [Configuration](configuration.md) | Files, folders and environment variables |
| [Troubleshooting](troubleshooting.md) | When something doesn't work |

## Inside eDock

| Page | What it covers |
|---|---|
| [Architecture](architecture.md) | The engine, the VM, the guest and the desktop app, and how they talk |
| [Development](development.md) | Building from source, the project layout, tests |
| [Releasing](releasing.md) | Versioning, signing, notarization, the `.dmg` and GitHub releases |
| [Licensing](licensing.md) | eDock's licences (MIT, and Apache-2.0 for the engine), and the Kiri fork it builds on |
