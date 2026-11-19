# Using docker with eDock

eDock's engine speaks the Docker Engine API on a unix socket:

```
~/.edock/run/docker.sock
```

Anything that talks to Docker can talk to eDock: the `docker` CLI, Compose, buildx, Testcontainers, IDE plugins, docker-py, and the Go and Java clients. You only need to point it there.

## The `edock` context

The simplest way is a docker context:

```sh
docker context create edock --docker host=unix://$HOME/.edock/run/docker.sock
docker context use edock
```

Or let `edock setup` do it and more (below): the app's **Engine › Command line › Set up** runs it. Switching between eDock and another engine is then `docker context use edock` or `docker context use orbstack`.

A single command can name the socket instead:

```sh
docker --host unix://$HOME/.edock/run/docker.sock ps
DOCKER_HOST=unix://$HOME/.edock/run/docker.sock docker compose up
```

## `edock setup`

```sh
/Applications/eDock.app/Contents/MacOS/edock setup [--use] [--no-agent] [--no-domains]
```

It makes this Mac use eDock the way an installer would, without touching anything another engine owns:

| What | Notes |
|---|---|
| An `edock` docker context | `--use` also makes it the current one |
| A LaunchAgent that starts `edock daemon` at login | `--no-agent` skips it |
| The `docker` CLI, Compose and buildx | Downloaded the first time, at the versions eDock pins, each checked against its SHA-256, into `~/Library/Application Support/eDock/cli`. No package manager needed. |
| `docker` and `edock` in `~/.edock/bin` | A marked line in your shell profiles (zsh, bash, fish) adds it to the **end** of PATH, so a docker CLI you already have keeps priority. Also `/usr/local/bin/edock` when that folder is yours. |
| `edock` as docker's default context | Only on a Mac with no other Docker engine |
| A buildx builder, `edock-build` | It loads what it builds into eDock (see [Builds](#builds)) |
| `*.edock.local` names for containers | Writes `/etc/resolver/edock.local` and asks for an administrator password once. `--no-domains` skips it; `--domains` does only this. |
| `/var/run/docker.sock` linked to eDock | See [below](#varrundockersock) |

`edock setup --uninstall` removes exactly those, plus the `edock` kubectl context.

## Compose

Compose works as usual: `up`, `down`, `ps`, `logs`, `exec`, several networks, and events. In the app, a project's containers are [grouped together](app/containers.md#compose-projects), and the whole project can be started, stopped, restarted or removed at once.

## Builds

`docker build` and `docker buildx build` run on BuildKit inside an eDock container. That is buildx's docker-container driver, through the `edock-build` builder. The builder is paused after 5 minutes without a build, and the next build resumes it (**Engine › Builds & import**, or `idle_secs` in `~/.edock/builder.json`).

On Apple silicon, amd64 build steps run through Rosetta.

Which builder a plain `docker build` uses:

```sh
edock build-with edock    # eDock's BuildKit builder
edock build-with docker   # docker's classic builder (DOCKER_BUILDKIT=0)
```

One known limit: `--add-host name:host-gateway` fails with BuildKit, because buildx refuses `host-gateway` for docker-container builders. Build steps reach the Mac as `host.docker.internal` without any flag; for another name, use the address itself (`--add-host name:198.19.248.254`), or `DOCKER_BUILDKIT=0`.

## amd64 images

Images built only for amd64 (Intel) run through **Rosetta inside the VM**. `--platform linux/amd64` is honoured. Most official images have an arm64 build, and those run natively.

## Container domains

Every container gets a name you can open in a browser without any setup:

```
http://<container-name>.edock.localhost
```

After `edock setup` (or `edock setup --domains`), containers also answer at:

- `http://<name>.edock.local`
- `<service>.<project>.edock.local` for Compose services
- the names in a container's `app.edock.domain=api.myapp` label, which sets its own names

HTTPS works too, with a certificate authority of eDock's own in `~/.edock/tls`. Its name constraints allow only `*.edock.local` and `*.edock.localhost`, so it can't vouch for any other site.

## Ports

`-p 8080:80` publishes on every interface, as Docker does. Use `-p 127.0.0.1:8080:80` to keep a port on the Mac only. Published ports show in the app, and a click opens them.

## Files from the Mac

- **Bind mounts** of Mac folders (`-v $PWD:/app`) use virtiofs.
- **Container files** are at `~/eDock` in Finder. This is a read-only NFS mount, always current.
- **The SSH agent** is forwarded at `/run/host-services/ssh-auth.sock`.

## `/var/run/docker.sock`

Some tools ignore docker contexts and use `/var/run/docker.sock` directly: docker-py, Go's `client.FromEnv`, docker-java, Testcontainers and IDE plugins. `edock setup --docker-socket` links that path to eDock's socket, but only when no other engine has it. It asks for an administrator password and installs a LaunchDaemon that puts the link back after a restart.

Until then, or if you decline the password, eDock points Testcontainers at itself through `~/.testcontainers.properties`, unless you have a file of your own there.

## Moving from OrbStack or Docker Desktop

```sh
edock import --list            # what would come over
edock import orbstack          # images, volumes and containers from OrbStack
edock import desktop-linux     # … or from Docker Desktop
```

## Docker's commands as `edock` commands

Docker's commands also work as `edock` commands, and these always go to eDock, whatever docker context is current:

```sh
edock run --rm alpine echo hi
edock ps
edock compose up -d
```
