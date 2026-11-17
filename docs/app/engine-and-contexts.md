# Engine and contexts

The app manages the containers of **one engine at a time**. That can be eDock's own engine, or any other engine that speaks the Docker Engine API on a unix socket: OrbStack, Docker Desktop, Colima, Podman, or a plain `dockerd`.

## Which engine the app shows

The toolbar shows the current engine:

- a green dot with its name and version while it answers;
- *stopped* when eDock's engine was turned off with **Stop**;
- *not running* when it doesn't answer.

Hover the dot to see its socket, or why it didn't answer.

The **picker** on the right of the toolbar switches engines. Each choice is a docker context:

| Context | Socket |
|---|---|
| `edock` | `~/.edock/run/docker.sock`. Always listed, even before eDock's engine has run. |
| `default` | `$DOCKER_HOST` when it's a unix socket, otherwise `/var/run/docker.sock` |
| Others | Every context in `~/.docker/contexts` with a unix socket: `orbstack`, `desktop-linux`, `colima`, … |

The app remembers your choice in `~/.edock/desktop.json`. It doesn't change docker's own current context; `docker context use` does that.

On its very first start, the app picks:

1. eDock's engine, if its socket is there;
2. otherwise docker's current context;
3. otherwise the first context it finds.

When the engine goes away or comes back, the app notices within two seconds, reconnects, and reloads every list.

## The Engine page

**Engine**, at the bottom of the sidebar, has six tabs. **Overview** shows eDock's engine, the command line setup, notifications, the version and the contexts. **Resources**, **Network**, **Builds & import** and **USB** are for eDock's own engine, and are greyed out while the app shows another. **AI agents** connects Claude, Cursor and VS Code to eDock.

### eDock engine

This part shows eDock's engine and which `edock` binary the app found, with a **Start** or **Stop** button. The engine is in one of four states:

| State | |
|---|---|
| **running** | On, with its VM up |
| **running · VM asleep** | On, with its VM asleep. It wakes on the next request, in about 0.15 s. |
| **stopped** | Turned off with **Stop**. Containers don't run until **Start**. |
| **not running** | No engine process at all |

- **Stop**, after asking, turns eDock off. Its containers stop and its VM powers off. The engine process stays, using about 15 MB, and answers that it's stopped, so nothing can start a container. Stop stays in force across restarts of the engine and of the Mac, until **Start**. **Start** starts again the containers that ran with a restart policy (`always`, `unless-stopped`).
  - It works however the engine was started: by the app (this run or an earlier one), by the LaunchAgent from `edock setup`, or by `edock daemon` in a terminal.
- **Start** turns a stopped engine on again and wakes its VM. When no engine process runs, it starts one first (`edock daemon`). Either way, it switches the app to eDock.

Containers with a restart policy (`--restart always`, `unless-stopped`) start again when the engine does.

### After an update

An update replaces the app, but the engine that was running keeps running the old version until it starts again. The Engine page notices when the engine's version differs from the app's, and offers **Restart engine**. That runs the engine from the updated app. A running engine comes back running, a stopped one stays stopped, and running containers stop and start again.

### Command line

**Set up** runs `edock setup` for you. It provides the `docker` CLI with Compose and buildx, without a package manager, plus `edock`, an `edock` docker context and the LaunchAgent. See [Getting started](../getting-started.md#use-it-from-the-terminal). Once it's done, the card says so; **Set up again** repairs it, for example after you've moved eDock.

Under the card are eDock's version and **Check for updates**. See [Updates](updates.md).

If another `edock` comes first on your PATH (an older copy in `/usr/local/bin` or `~/.local/bin`, say), the card warns and names it: terminals run that one, not this eDock's, so an update seems not to have happened. eDock never removes it; delete it, or put `~/.edock/bin` before its folder in your PATH.

The engine's output goes to `~/.edock/desktop-engine.log`.

The app looks for the engine binary in this order:

1. `$EDOCK_ENGINE`;
2. next to the app's own binary (inside `eDock.app`, the normal case);
3. `~/.edock/bin/edock`;
4. `engine/target/release/edock` in a source checkout (development).

### Notifications

**Tell me when a container runs out of memory or turns unhealthy** (on by default) shows a macOS notification when the kernel kills a container for using all the memory it may have, or when its health check starts failing. Click it for the container's log. Each container gets at most one of each every ten minutes, and eDock's own containers (Linux machines, Kubernetes, helpers) none. It works on any engine the app shows, as long as the app runs. With them off, the [menu bar](menu-bar.md) still marks such a container, with a dot and in its popover.

### Contexts

Every context, with its socket, and which one docker itself uses (*docker's current*). **Use** switches the app to it.

### The engine's facts

For the engine the app is on, while it answers:

- version, OS and architecture;
- CPUs and memory;
- how many containers there are, and how many run;
- the number of images.

eDock's engine reports no memory while its VM sleeps.

## Resources

**Now** shows:

- the VM, awake or asleep, with its CPUs and memory;
- the memory the containers use, out of the most the VM may take;
- the VM's CPU use;
- the engine's own memory on the Mac;
- the guest agent's memory in the VM;
- processes the VM had to kill for lack of memory, if any.

**Size** sets the VM's CPUs, and the most memory it may take. Memory the containers don't use goes back to macOS while the VM runs, and all of it when it sleeps, so a generous cap costs nothing until it's used.

The new size applies from the VM's next start. While the VM still runs with its old size, **Restart VM** restarts it now; running containers stop, and those with a restart policy come back. The size is kept in `~/.edock/vm.json`. `EDOCK_CPUS` and `EDOCK_MEMORY_MIB` override it.

## Network

**Container domains** shows where containers answer (`http://<name>.edock.localhost`, and `<service>.<project>…` for Compose), and the ports. Change the ports there when another server holds 80 and 443; they're kept in `~/.edock/domains.json`.

**HTTPS**: **Trust HTTPS** has macOS trust eDock's certificate authority, so `https://<name>.edock.localhost` shows a padlock. macOS asks for your password. The authority's certificates can only vouch for `*.edock.local` and `*.edock.localhost`, never any other site. **Stop trusting** undoes it. The same from a terminal: [`edock tls`](../cli.md#edock-tls).

**Proxy**, for networks that need one for image pulls, pushes, searches and logins:

| Setting | |
|---|---|
| macOS's settings | The default; it shows the proxy in use now |
| Custom | An HTTP and an HTTPS proxy, and the hosts that go direct |
| None | No proxy |

**Pass it to new containers** gives them `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY`, as Docker Desktop does. VPNs need nothing here. It's kept in `~/.edock/proxy.json`; see [Configuration](../configuration.md#proxies).

## Builds & import

- **Builds** sets how big `docker build`'s cache may grow, and how long BuildKit waits without a build before it stops. **Empty cache** frees all of it; the next builds start from scratch.
- **docker build uses** picks the builder for a plain `docker build`, `docker compose build` and `docker buildx build`:
  - **docker's context** (the default when another engine is on the Mac): builds go where docker's context points, so OrbStack's stay OrbStack's.
  - **eDock, always**: every build runs on eDock's BuildKit and the image lands in eDock, whatever the context. Everything else (`docker run`, `ps`, `compose up`) still follows the context. Choosing **docker's context** again puts back exactly what eDock changed in `~/.docker`.
- **Speed test** times a container's whole life (create, start, run `true`, exit, removed) 20 times on each engine running here: eDock, OrbStack, Docker Desktop. It shows the median and the slowest 10%, and how many times eDock's the others take. It pulls `alpine` where it's missing, and removes every container it makes.
- **Import** brings images, volumes and containers over from OrbStack or Docker Desktop, while that engine runs.
  - It shows how many of each eDock doesn't have yet.
  - The other engine is only read.
  - Volumes keep their files' owners, and containers arrive stopped.
- **Diagnostics** saves a report of eDock's state and recent logs, with tokens taken out, where you choose. Nothing is sent anywhere. **Engine log** shows the engine's last 500 lines, or only its warnings and errors.

## USB (beta)

**USB** lists the USB devices on the Mac: serial adapters, Zigbee and Z-Wave sticks, development boards, HID devices. It looks again every few seconds, so a device shows up when you plug it in.

- **Hand to eDock** passes the device through to eDock's VM. It's gone from the Mac until you give it back. The first time, macOS asks for an administrator's password to install eDock's USB helper.
- Once it's in eDock, the card shows its path in the VM (`/dev/ttyACM0`, say) and a `docker run --device …` to copy. Containers reach it with `--device`, as on Linux.
- **Give back to the Mac** returns it.

A device that resets, or drops off while the Mac sleeps, is handed over again when it comes back. The same from a terminal: [`edock usb`](../cli.md#edock-usb-beta).

## AI agents

**AI agents** shows how to add eDock as an MCP server to Claude Code, Claude Desktop, Cursor and VS Code, with the command and the settings to copy. See [AI agents](ai-agents.md).
