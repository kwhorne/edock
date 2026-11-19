# Features

What eDock supports today, through the `edock` Docker context.

## Platforms

Apple silicon and Intel Macs (macOS 14+), and Linux hosts (no VM; built with
`cargo xtask linux`). Windows support (kiri-win) was dropped in the eDock fork.

## What works

- **Containers**: `run` (`--rm`, `-d`, `-it`, `-e`, `-u`, `-w`, `--name`,
  `--restart`, `--entrypoint`), `exec`, `logs`, `attach`, `stop`, `kill`,
  `restart`, `rm`, `wait`, `rename`, `pause`/`unpause`, `update`, `stats`,
  `top`, `diff`, `export`, `commit`, `cp` (both ways, running or not),
  `container prune`; records survive a daemon restart; filters on every list
- **Limits and security**: `--memory`, `--cpus`, `--cpu-shares`,
  `--pids-limit`, `--shm-size`; Docker's default capabilities and a default
  seccomp filter; `--cap-add`/`--cap-drop`, `--privileged`, `--read-only`,
  `--security-opt seccomp=unconfined` and `no-new-privileges`. Even
  privileged and unconfined containers keep the host boundary: no vsock
  sockets (the way to the Mac) and no io_uring
- **Networking**: a default bridge, user networks with DNS by name and
  alias, `network connect`/`disconnect`, `--network host|none`, `-p`/`-P`
  (TCP and UDP) on all interfaces (as Docker does; `-p 127.0.0.1:8080:80`
  keeps a port local), `--add-host`, `host.docker.internal`,
  `--network container:<name>` (Compose `network_mode: service:x`; as with
  Docker, if that container restarts, the ones sharing its network keep the
  old, now empty namespace until they are restarted too), IPAM (`--subnet`,
  `--gateway`, `--ip-range`, `--aux-address`), `--ip`, `--mac-address`,
  `--dns`, `--dns-search`, `--dns-option`, `--domainname`, `--link`,
  `--ipv6` (Compose `enable_ipv6`: an IPv6 subnet beside the IPv4 one, from
  `--subnet` or a /64 of eDock's own under fd00::/8; the default bridge stays
  IPv4-only, as Docker's does), and
  `http://<name>.edock.local` once you turn it on (`edock setup --domains`), or `http://<name>.edock.localhost` with no
  setup. Compose services answer at `<service>.<project>.edock.local`, and a
  `app.edock.domain=api.myapp` label picks a container's own names; HTTPS too
  (`https://<name>.edock.local`), with eDock's own certificate authority in
  `~/.edock/tls`, whose name constraints allow only `*.edock.local` and
  `*.edock.localhost`
- **Storage**: named and anonymous volumes, bind mounts of Mac paths
  (virtiofs), `--tmpfs`, `--mount`, `volume` commands
- **Images**: `pull` (with `docker login` credentials), `push`, `tag`,
  `save`, `load`, `history`, `search`, `image prune`, `system df`;
  **amd64 images run through Rosetta**, `--platform` honoured
- **Builds**: `docker build` / `buildx build` through BuildKit running in a
  eDock container (buildx's docker-container driver), stopped when idle;
  on Apple silicon, amd64 build steps run through Rosetta too
- **Compose**: `up`, `down`, `ps`, `logs`, `exec`, multiple networks, events
- **USB devices (beta)**: `edock usb attach` hands a device (a serial
  adapter, a Zigbee stick, a development board) to your containers on Mac
  and Linux. On a Mac, a device that drops off the bus while attached (the board
  resets, or the Mac sleeps) is attached again when it comes back
- **Mac integration**: container files at `~/eDock` in Finder (read-only
  NFS, always current), SSH agent forwarding at
  `/run/host-services/ssh-auth.sock`
- **Memory**: a balloon gives unused memory back while running, and the VM
  sleeps after 60 s idle (the daemon stays at ~15 MB; waking takes ~0.15 s)
- **Resources**: the VM's CPUs and memory (`~/.edock/vm.json`, used from
  the VM's next start; `EDOCK_CPUS` and `EDOCK_MEMORY_MIB` override it)
- **Kubernetes**: `edock k8s start` — a single-node k3s cluster in one
  container, ready in ~20 s, as the `edock` kubectl context; `edock k8s load
  IMAGE` puts an image built or pulled on eDock into it, with no registry
- **Linux machines**: `edock machine create dev --distro ubuntu` and
  `edock machine shell dev` — your user, sudo, and `/Users` at the same path;
  `edock machine ssh on` makes `ssh dev.edock` (and VS Code, Cursor, Zed) reach
  them, through the engine, with no sshd left running
- **Linux hosts**: the same `edock` with no VM — the agent runs on the host's
  kernel (`cargo xtask linux`, `contrib/linux/edock.service`); arm64 and
  x86-64; `tests/linux-host.sh` checks it. Membership of the `edock` group is
  root-equivalent, exactly like the `docker` group

Kubernetes LoadBalancer services are published on `localhost`, and
`edock machine create NAME --systemd` boots systemd as the machine's init.

## Known issues

- **Mac: an attached USB device leaves your containers while the Mac
  sleeps.** macOS takes USB devices off the bus when it sleeps, so a device
  such as `/dev/ttyACM0` is gone from containers until the Mac wakes. eDock
  then attaches it again by itself. eDock doesn't keep the Mac awake for a
  device, since that would drain a laptop's battery. For a long job that
  runs unattended (flashing boards, logging a serial port), keep the Mac
  awake while it runs: `caffeinate -s` in a terminal (on power), or turn on
  "Prevent automatic sleeping when the display is off" in System Settings
  (Battery › Options on a laptop, Energy on a desktop Mac).

- **`docker build --add-host name:host-gateway` with buildx fails** with
  `host-gateway is not supported by the docker-container driver`. buildx
  checks this on your Mac, before the build reaches eDock, and refuses
  `host-gateway` for every builder except one inside Docker's own engine;
  eDock's builder is a docker-container builder, as are the ones you make
  with `docker buildx create` on any Docker. Build steps reach the Mac
  anyway: `host.docker.internal` resolves to it with no flag at all, and
  `--add-host name:198.19.248.254` (the Mac's address from the VM; on a
  Linux host, `172.18.0.1`) gives it another name. The classic builder
  (`DOCKER_BUILDKIT=0`, or a docker CLI without buildx) takes
  `host-gateway` as it is. A builder inside eDock's engine, which buildx
  would let map it, is planned.
