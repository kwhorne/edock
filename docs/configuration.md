# Configuration

## Where eDock keeps things

| Path | What |
|---|---|
| `~/.edock/run/docker.sock` | The engine's Docker API socket |
| `~/.edock/` | The engine's state, logs and settings, and the app's settings |
| `~/.edock/bin/` | `edock` and `docker`, after `edock setup` |
| `~/Library/Application Support/eDock/cli/` | The `docker` CLI, Compose and buildx that `edock setup` downloaded |
| `~/.edock/tls/` | The certificate authority for `https://*.edock.local` |
| `~/Library/Application Support/eDock/` | Images, volumes and the VM's data disk (`vm/data.img`) |
| `~/eDock` | Container files, in Finder (read-only) |
| `~/Library/LaunchAgents/app.edock.daemon.plist` | Starts the engine at login, after `edock setup` |

The VM's data disk is a sparse file. It takes only the space that images and volumes use, up to 64 GiB.

## Settings files

All of these are in `~/.edock/`:

| File | Read by | What |
|---|---|---|
| `vm.json` | The engine, at each VM start | `{"cpus": 4, "memory_mib": 8192}`: the VM's size |
| `k8s.json` | `edock k8s` | The cluster's `lan`, `tls_san` and `port`. Change them with `edock k8s set`. |
| `builder.json` | The engine | The BuildKit builder: `{"idle_secs": 300, "cache_gb": 20}` |
| `proxy.json` | The engine | The proxy for pulls, pushes and logins (below) |
| `desktop.json` | The app | Which context the app shows |
| `stopped` | The engine | There while eDock is stopped with **Stop**, so the engine stays off across restarts until **Start** |

## Proxies

Image pulls, pushes, searches and logins go through a proxy when your network needs one. `~/.edock/proxy.json` picks it:

```json
{ "mode": "system" }
```

| `mode` | |
|---|---|
| `system` | macOS's own proxy settings (the default) |
| `custom` | `"http"`, `"https"` and `"no_proxy"` from the file |
| `none` | No proxy |

With `"containers": true`, new containers also get `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY`, as Docker Desktop does. VPNs need nothing here: container traffic leaves through macOS's own networking.

## Environment variables

### The engine (`edock daemon`)

| Variable | Default | What |
|---|---|---|
| `EDOCK_SOCKET` | `~/.edock/run/docker.sock` | The API socket's path |
| `EDOCK_DATA_DIR` | `~/Library/Application Support/eDock` | Images, volumes and the VM's disk |
| `EDOCK_CPUS` | from `vm.json` | The VM's CPUs (overrides `vm.json`) |
| `EDOCK_MEMORY_MIB` | from `vm.json` | The VM's memory in MiB (overrides `vm.json`) |
| `EDOCK_IDLE_SECS` | `60` | Seconds idle before the VM sleeps; `0` keeps it awake |
| `EDOCK_HTTP_PORT` | `80` | The port for `http://<name>.edock.localhost` |
| `EDOCK_HTTPS_PORT` | `443` | The port for `https://<name>.edock.local` |
| `EDOCK_NO_FINDER` | unset | `1`: don't mount container files at `~/eDock` |
| `EDOCK_BUILDER_IDLE_SECS` | `300` | Seconds without a build before the builder stops |
| `EDOCK_BUILD_CACHE_GB` | by disk size | The builder's cache limit in GB |
| `EDOCK_GUEST_DIR` | inside the app | Where the kernel and initramfs are (development) |
| `EDOCK_NO_BALLOON` | unset | `1`: don't give unused memory back to macOS while the VM runs |

To run the engine without touching ports 80 and 443 or Finder, for example while another tool has those ports:

```sh
EDOCK_NO_FINDER=1 EDOCK_HTTP_PORT=8081 EDOCK_HTTPS_PORT=8443 edock daemon
```

The engine also uses `docker login`'s credentials (in `DOCKER_CONFIG`, else `~/.docker`) for pulls and pushes.

### The app

| Variable | What |
|---|---|
| `EDOCK_ENGINE` | The `edock` binary the app starts (see [Engine and contexts](app/engine-and-contexts.md#edock-engine)) |
| `DOCKER_HOST` | Used for the `default` context when it's a unix socket |

The app passes its environment on to an engine it starts, so the engine variables above work there too:

```sh
EDOCK_NO_FINDER=1 open -a eDock
```

Note that `open` only passes variables to an app that isn't already running.
