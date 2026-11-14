# The `edock` CLI

`edock` is eDock's engine and its command line, in one binary:

- **Inside the app:** `/Applications/eDock.app/Contents/MacOS/edock`.
- **After [`edock setup`](docker.md#edock-setup):** also `~/.edock/bin/edock`, which is on your PATH.

```
edock COMMAND
```

## Docker's commands

```sh
edock run --rm alpine echo hi
edock ps -a
edock compose up -d
edock build -t app .
```

Any docker command (`run`, `ps`, `exec`, `logs`, `stop`, `rm`, `images`, `pull`, `build`, `compose`, …) runs the docker CLI against **eDock's socket**, whatever docker context is current. It needs a `docker` CLI on PATH.

Before it runs, `edock` checks that the engine runs the same version as itself, and says on stderr when it doesn't.

## `edock daemon`

```sh
edock daemon
```

Runs the engine: the Docker Engine API on `~/.edock/run/docker.sock`. The VM boots on the first request that needs it, and sleeps after 60 seconds idle. The app's **Start** button and the LaunchAgent from `edock setup` both run this. Stop it with `SIGTERM`.

See [Configuration](configuration.md#environment-variables) for the environment variables it reads.

## `edock setup`

```sh
edock setup [--use] [--no-agent] [--no-domains]
edock setup --domains
edock setup --docker-socket
edock setup --uninstall
```

Sets this Mac up to use eDock. It covers:

- a docker context;
- the LaunchAgent;
- the `docker` CLI, Compose and buildx, downloaded and checksum-checked the first time;
- `~/.edock/bin` on PATH;
- a buildx builder;
- `*.edock.local` names;
- `/var/run/docker.sock`.

The app's **Engine › Command line › Set up** runs `edock setup --no-domains`, plus `--use` when ticked. [Using docker with eDock](docker.md#edock-setup) describes each part.

| Flag | |
|---|---|
| `--use` | Make `edock` docker's current context |
| `--no-agent` | No LaunchAgent: the engine doesn't start at login |
| `--no-domains` | Skip `*.edock.local`, which needs an administrator password |
| `--domains` | Only set up `*.edock.local` |
| `--docker-socket` | Only link `/var/run/docker.sock` to eDock |
| `--uninstall` | Take back everything `setup` did, and the `edock` kubectl context |

## `edock k8s`

```sh
edock k8s start [--use]
edock k8s stop
edock k8s status
edock k8s delete
edock k8s set [--lan on|off] [--tls-san NAME,…] [--port N]
```

A single-node Kubernetes cluster (k3s) in the `edock-k8s` container. See [Kubernetes](app/kubernetes.md).

| Command | |
|---|---|
| `start` | Creates the cluster or starts it again, waits until the node is ready, and writes the `edock` kubectl context. `--use` makes that context kubectl's current one. |
| `stop` | Stops it; its state is kept |
| `status` | Its state, the API server's port and `kubectl get nodes` |
| `delete` | Deletes it with all its state, and the kubectl context |
| `set --lan on` | The API server and LoadBalancer services on every interface |
| `set --tls-san A,B` | Extra names or addresses in the API server's certificate |
| `set --port N` | The Mac port for the API server (1024 or higher; default 6443) |

`set` recreates the node container with the new settings, and keeps the cluster's state. The settings are in `~/.edock/k8s.json`.

## `edock tls`

```sh
edock tls status
edock tls trust
edock tls untrust
```

Whether macOS trusts eDock's certificate authority for websites, so `https://<name>.edock.localhost` has a padlock. `trust` asks for your password in macOS's own prompt. The authority can only vouch for `*.edock.local` and `*.edock.localhost`. The app's **Engine › Domains › Trust HTTPS** runs it.

## `edock build-with`

```sh
edock build-with edock    # docker build uses eDock's BuildKit builder
edock build-with docker   # docker build uses docker's classic builder
```

Which builder a plain `docker build` uses. See [Builds](docker.md#builds).

## `edock import`

```sh
edock import [orbstack|desktop-linux] --list
edock import [orbstack|desktop-linux]
edock import orbstack image NAME | volume NAME | container NAME
```

Brings images, volumes and containers over from OrbStack (the default) or Docker Desktop (`desktop-linux`).

- `--list` shows what there is, and what eDock has already.
- With `image`, `volume` or `container NAME`, it brings over that one only.

The source is only read: images through `docker save`, and volumes through a short-lived read-only helper container, so file owners are kept. Containers arrive stopped.

## `edock machine`

```sh
edock machine create NAME [--distro D | --image IMAGE] [--systemd]
edock machine list
edock machine shell NAME [-- CMD...]
edock machine start | stop | restart | delete NAME
```

Long-lived Linux environments on eDock. Each one has your user, `sudo`, and `/Users` at the same path as on the Mac.

| Flag | |
|---|---|
| `--distro` | `ubuntu` (the default), `debian`, `alpine`, `fedora`, `arch`, `rocky` or `opensuse` |
| `--image` | Any image to start from, instead of a distribution |
| `--systemd` | Boots systemd as the machine's init |

`shell NAME -- CMD…` runs one command instead of a shell.

## `edock usb` (beta)

```sh
edock usb list
edock usb attach DEVICE
edock usb detach DEVICE
```

Hands a USB device to your containers (a serial adapter, a Zigbee stick, a development board). On a Mac it's passed through to the VM. A device that drops off the bus while attached is attached again when it comes back, for example when the board resets or the Mac sleeps and wakes.

## `edock diag`

```sh
edock diag logs [-f] [--engine|--cli] [--errors] [-n N]
edock diag report [--out PATH]
```

| Command | |
|---|---|
| `logs` | eDock's own logs: `-f` follows, `--engine` or `--cli` picks one, `--errors` keeps the errors, `-n` sets how many lines |
| `report` | Writes a diagnostics report to a local file, with tokens taken out. Nothing is sent anywhere. |

## `edock bench`

```sh
edock bench [--runs N] [--image IMAGE] [--network none|bridge]
```

Times container starts on eDock and on the other engines it finds.

## `edock boot` and `edock raw`

```sh
edock boot [--console] [--pings N]
edock raw run [--rm] [-e KEY=VALUE]... IMAGE [COMMAND...]
edock raw pull IMAGE | raw images | raw rmi IMAGE...
```

Low-level tools that boot a guest of their own, without the daemon. They're for development and debugging.
