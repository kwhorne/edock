# Engine and contexts

The app manages the containers of **one engine at a time**. That can be eDock's own engine, or any other engine that speaks the Docker Engine API on a unix socket: OrbStack, Docker Desktop, Colima, Podman, or a plain `dockerd`.

## Which engine the app shows

The toolbar shows the current engine:

- a green dot with its name and version while it answers;
- *not running* when it doesn't.

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

**Engine**, at the bottom of the sidebar, has four tabs. **Overview** shows eDock's engine, the command line setup, the version and the contexts. **Resources**, **Network** and **Builds & import** are for eDock's own engine.

### eDock engine

This part shows eDock's engine and which `edock` binary the app found, with a **Start** or **Stop** button. The engine is in one of four states:

| State | |
|---|---|
| **running** | On, with its VM up |
| **running · VM asleep** | On, with its VM asleep. It wakes on the next request, in about 0.15 s. |
| **stopped** | Turned off with **Stop**. Containers don't run until **Start**. |
| **not running** | No engine process at all |

- **Stop**, after asking, turns eDock off. Its containers stop and its VM powers off. The engine process stays, using about 15 MB, and answers that it's stopped, so nothing can start a container. Stop stays in force across restarts of the engine and of the Mac, until **Start**.
  - It works however the engine was started: by the app (this run or an earlier one), by the LaunchAgent from `edock setup`, or by `edock daemon` in a terminal.
- **Start** turns a stopped engine on again and wakes its VM. When no engine process runs, it starts one first (`edock daemon`). Either way, it switches the app to eDock.

Containers with a restart policy (`--restart always`, `unless-stopped`) start again when the engine does.

### After an update

An update replaces the app, but the engine that was running keeps running the old version until it starts again. The Engine page notices when the engine's version differs from the app's, and offers **Restart engine**. That runs the engine from the updated app. A running engine comes back running, a stopped one stays stopped, and running containers stop and start again.

### Command line

**Set up** runs `edock setup` for you. It provides the `docker` CLI with Compose and buildx, without a package manager, plus `edock`, an `edock` docker context and the LaunchAgent. See [Getting started](../getting-started.md#use-it-from-the-terminal). Once it's done, the card says so; **Set up again** repairs it, for example after you've moved eDock.

Under the card are eDock's version and **Check for updates**. See [Updates](updates.md).

The engine's output goes to `~/.edock/desktop-engine.log`.

The app looks for the engine binary in this order:

1. `$EDOCK_ENGINE`;
2. next to the app's own binary (inside `eDock.app`, the normal case);
3. `~/.edock/bin/edock`;
4. `engine/target/release/edock` in a source checkout (development).

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

The new size applies from the VM's next start. **Restart VM** starts it now; running containers stop, and those with a restart policy come back. The size is kept in `~/.edock/vm.json`. `EDOCK_CPUS` and `EDOCK_MEMORY_MIB` override it.

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
- **Import** brings images, volumes and containers over from OrbStack or Docker Desktop, while that engine runs.
  - It shows how many of each eDock doesn't have yet.
  - The other engine is only read.
  - Volumes keep their files' owners, and containers arrive stopped.
- **Diagnostics** saves a report of eDock's state and recent logs, with tokens taken out, where you choose. Nothing is sent anywhere.
