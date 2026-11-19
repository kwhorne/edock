# Development

Building the eDock engine from source, running it, and how it fits together.

## Build

Needs Rust (with the `aarch64-unknown-linux-musl` target for the guest:
`rustup target add aarch64-unknown-linux-musl`) and, for the kernel and disk
template, a Docker engine.

```sh
cargo xtask kernel    # guest/out/Image
cargo xtask disk      # guest/out/data.img (sparse 64 GiB ext4 template)
cargo xtask guest     # guest/out/initramfs.cpio
cargo xtask host      # target/release/edock, signed with the virtualization entitlement
cargo xtask bundle    # host + target/edock-engine/{MacOS,Resources} for the desktop app
```

For Intel Macs add `--arch x86_64` to `kernel`, `guest`, `host` and `bundle`:
an x86-64 guest in `guest/out/x86_64` (a `bzImage`, cross-compiled on Apple
Silicon) and the host for `x86_64-apple-darwin`
(`rustup target add x86_64-apple-darwin x86_64-unknown-linux-musl`). On
Intel there is no Rosetta for Linux: amd64 images run natively, and images
with no amd64 build are refused.

## Where the binary finds the guest

`edock` looks for `Image` (`bzImage` on Intel) and `initramfs.cpio`:

1. in `$EDOCK_GUEST_DIR`, when set;
2. in `../Resources/guest` next to the binary (after resolving symlinks), i.e.
   `Some.app/Contents/Resources/guest` when the binary is
   `Some.app/Contents/MacOS/edock`;
3. in `guest/out` of the checkout the binary was built in (development).

The data disk is copied on first boot into `<data dir>/vm/data.img` from
`data.img` in that folder (an APFS clone), or unpacked from `data.img.tgz`
(a sparse tarball; `cargo xtask bundle` makes one).

## Use

```sh
./target/release/edock daemon &      # serves ~/.edock/run/docker.sock; the VM boots on first use
docker --host unix://$HOME/.edock/run/docker.sock run --rm alpine echo hello
docker context create edock --docker host=unix://$HOME/.edock/run/docker.sock   # optional
./target/release/edock bench         # eDock vs other engines
SOAK=600 tests/e2e.sh                # every check, then 600 runs watching for leaks
tests/parity.py                      # the same docker commands on eDock and on Moby (OrbStack), compared
tests/upgrade.py                     # released engines' data, opened by this one
```

`tests/upgrade.py` has each of a few released engines (0.2.0 to the latest)
make some state, with a HOME of its own under `$TMPDIR`: containers running and
stopped, restart policies, a volume with files, a network, a Compose project
with data, a committed image, and a parked project where it has On demand.
Then this checkout's engine starts on the same data, and it checks that all of
it came back and works, and that its log has no errors. It also opens a
`containers.json` with a record it can't read. Releases are fetched with `gh`.

`tests/parity.py` runs each of its cases (users and groups, exit codes, `exec`,
`cp`, `inspect`, DNS, volumes, capabilities, ports, and more) against eDock and
against an engine that runs Moby, Docker's own (OrbStack by default; `--b
SOCKET` for another), with ids, times and addresses made equal, and shows where
the answers differ. A difference with a good reason goes in its `KNOWN`, with
the reason; any other fails it. `-k NAME` runs some, `-v` shows every answer.

Docker's commands also work as `edock` commands, always on eDock whatever
docker context is selected: `edock run`, `edock ps`, `edock exec`, `edock logs`,
`edock build`, `edock compose up` and the rest run the docker CLI against
eDock's socket. (`edock import` is eDock's own: it brings images, volumes and
containers over from OrbStack or Docker Desktop.) The daemon-free tools
that boot their own guest are `edock raw run|pull|images|rmi`.

`edock` keeps its data in `~/Library/Application Support/eDock` (override with
`EDOCK_DATA_DIR`) and its socket, logs and settings in `~/.edock` (socket path
override: `EDOCK_SOCKET`). It does not touch `/usr/local/bin/docker`.
`/var/run/docker.sock` changes only when you ask: `edock setup --docker-socket`
(or the password prompt for `*.edock.local` names) links it to eDock's socket
when no other engine has it, after macOS asks for an administrator password,
and installs `/Library/LaunchDaemons/app.edock.docker-socket.plist` to put the
link back after a restart. A socket another engine owns is never replaced.

The daemon mounts container files at `~/eDock` (Finder, read-only NFS) unless
`EDOCK_NO_FINDER=1`, and serves container names on ports 80/443 unless
`EDOCK_HTTP_PORT` / `EDOCK_HTTPS_PORT` say otherwise.

## Architecture

```
docker CLI ─┐
compose ────┼─ ~/.edock/run/docker.sock ─► edock daemon ── vsock ──► guest agent (PID 1)
buildx ─────┤     Docker Engine API           owns the VM            clone3 → container
desktop app ┘
```

- **VM**: Apple's Virtualization.framework, driven from Rust through objc2.
  A custom 10 MB kernel (`guest/kernel/`) built from allnoconfig, with one
  patch that cuts virtio-pci reset waits from a scheduler tick to
  microseconds, one that hands big TCP to the host as GSO, one that keeps
  newly created FUSE files owner-readable, and an upstream GRO fix that keeps
  forwarded packets segmentable. Rosetta is shared in over virtiofs and registered with
  binfmt_misc.
- **Guest agent**: a ~900 KB static Rust binary that is PID 1. One `poll()`
  loop serves the host, reaps children, forwards container output, relays
  ports, answers DNS and runs DHCP. `docker cp` and friends run in forked
  children so a large copy never stalls it.
- **Containers**: one `clone3` straight into new namespaces and the
  container's cgroup (`CLONE_INTO_CGROUP`), overlayfs over layers unpacked
  on an ext4 data disk, `pivot_root`, capabilities and a seccomp filter.
  Networking is set up with hand-written rtnetlink and nf_tables messages.
  No runc, no shim, no dockerd.
- **Images**: pulled and pushed on the Mac (`crates/edock-image`), streamed
  into the guest once and unpacked there.

## Layout

| Path | What |
|------|------|
| `crates/edock` | the `edock` binary: daemon, Docker API, CLI, bench |
| `crates/edock-vmm` | Virtualization.framework VM |
| `crates/edock-init` | guest PID 1, agent and container runtime |
| `crates/edock-proto` | host ↔ guest wire protocol (no_std, no allocation per message) |
| `crates/edock-image` | image references, registry client, blob store, archives |
| `guest/kernel` | kernel config and patches |
| `tests/e2e.sh` | ~350 checks through the real `docker` CLI, plus a leak soak (`K8S=1` adds Kubernetes) |
| `tests/linux-host.sh` | the Linux host build, driven in a privileged container |
| `tests/finder-mount.sh` | the Mac's Finder mount at `~/eDock`: covers only an empty folder or its own old mount, never your files or another mount |
| `contrib/linux` | systemd unit for a Linux host |
| `xtask` | build tasks |
