# Troubleshooting

## Where to look first

```sh
edock diag logs --errors -n 50     # the engine's recent errors
tail -50 ~/.edock/desktop-engine.log   # the output of an engine the app started
edock diag report                  # a full report in a local file (tokens removed)
```

## The app says the engine isn't running

- **On eDock's context:** start the engine on the **Engine** page, or in the menu bar item's menu.
  - If **Start** is greyed out, the app found no `edock` binary. Reinstall eDock, or set `EDOCK_ENGINE`.
  - If it starts and stops again, read `~/.edock/desktop-engine.log`.
- **On another context** (OrbStack, Docker Desktop): that engine isn't running. Start it, or switch the app to eDock with the toolbar's picker.
- **A context marked *no socket*:** its engine has never run, or it was uninstalled.

## Stopping the engine

Quitting eDock leaves the engine as it is, so `docker` keeps working. To turn it off, use **Stop** on the Engine page, or **Stop eDock engine** in the menu bar item's menu. This works however the engine was started. Its containers stop, its VM powers off, and it stays off, even across restarts, until **Start**.

A stopped engine still has its process, using about 15 MB, which answers that eDock is stopped. To end the process too:

```sh
pkill -TERM -f "edock daemon"
```

If the LaunchAgent from `edock setup` started it, it starts again at login, but stays stopped until **Start**. To keep it from starting at all, use `edock setup --uninstall`, or turn eDock off under System Settings › General › Login Items.

## `eDock is stopped: press Start…`

Every docker command answers this while eDock is stopped. Press **Start** on the Engine page, or in the menu bar item's menu.

## `docker: command not found` after setting up

`edock setup` adds `~/.edock/bin` to your PATH in your shell's profile. A terminal window that was already open doesn't see it: open a new one. If it still isn't found, check that the marked line is in `~/.zshrc` (or `~/.bash_profile`, or fish's `config.fish`), and press **Set up again** on the Engine page.

## Ports 80 or 443 are taken

The engine serves `*.edock.localhost` on ports 80 and 443. If another tool holds them (a local web server, another container engine), move eDock's:

```sh
EDOCK_HTTP_PORT=8081 EDOCK_HTTPS_PORT=8443 edock daemon
```

Containers' own published ports (`-p 8080:80`) don't depend on this.

## `docker` still talks to another engine

`docker context ls` shows which context is current (marked `*`). Switch with `docker context use edock`, or run `edock setup --use` once.

The app's picker doesn't change docker's context, and `docker context use` doesn't change the app's.

## A tool ignores the docker context

docker-py, Testcontainers, IDE plugins and other tools that read `/var/run/docker.sock` directly don't follow contexts. Either:

- set `DOCKER_HOST=unix://$HOME/.edock/run/docker.sock` for that tool, or
- run `edock setup --docker-socket` to link `/var/run/docker.sock` to eDock, when no other engine owns it.

## `docker build --add-host …:host-gateway` fails

buildx refuses `host-gateway` for docker-container builders, and eDock's builder is one. Use the address itself (`--add-host name:198.19.248.254`), or `DOCKER_BUILDKIT=0 docker build …`.

## An amd64 image is slow

amd64 images run through Rosetta. That's fast, but slower than an arm64 image. Check whether the image has an arm64 build: `docker manifest inspect IMAGE`. Most official images do.

## Updates

**"Couldn't check for updates".** The check needs to reach `github.com`. Behind a proxy, see [Configuration](configuration.md#proxies).

**Installing refuses.** eDock replaces itself only where it's allowed to:

- **Opened from the disk image or Downloads:** macOS runs it from a temporary, read-only copy. Move eDock to Applications first.
- **In a folder you can't write to**, such as another account's Applications: install it in `~/Applications` instead.
- **A development build** (`target/…`) never updates itself.

**The engine runs an older eDock.** Press **Restart engine** on the Engine page after an update.

## Kubernetes

**The page offers to switch to eDock.** The cluster runs on eDock's engine; the app is on another one.

**`Start` takes long the first time.** The first start pulls k3s. Later starts take seconds.

**kubectl didn't answer.** The node may still be starting, or be short of memory. Give the VM more in `~/.edock/vm.json` (see [Configuration](configuration.md#settings-files)), then:

```sh
edock k8s status
docker --context edock logs edock-k8s
```

**Port 6443 is taken.** The cluster takes the next free port, and the page shows which. Pick one with `edock k8s set --port 6550`.

**Start from scratch:** `edock k8s delete`, then **Start** again. This deletes everything deployed in the cluster.

## A USB device disappears while the Mac sleeps

macOS takes USB devices off the bus when it sleeps. eDock attaches them again when the Mac wakes. For long unattended jobs, keep the Mac awake (`caffeinate -s`).

## Starting over

To remove everything eDock made:

```sh
edock k8s delete                              # if you made a cluster
edock setup --uninstall                       # context, LaunchAgent, PATH line, builder, domains, socket link
pkill -TERM -f "edock daemon"
rm -rf ~/.edock "$HOME/Library/Application Support/eDock"
```

Then drag eDock from Applications to the Bin.
