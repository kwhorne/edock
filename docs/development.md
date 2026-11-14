# Development

## Layout

```
eDock/
├── engine/       the engine: a fork of Kiri (Rust workspace)
│   ├── crates/edock         the edock binary: daemon, Docker API, CLI
│   ├── crates/edock-vmm     Virtualization.framework VM
│   ├── crates/edock-init    guest PID 1: agent and container runtime
│   ├── crates/edock-proto   host ↔ guest protocol
│   ├── crates/edock-image   images: references, registry client, blob store
│   ├── guest/kernel         kernel config and patches
│   ├── xtask                build tasks
│   └── tests                end-to-end tests through the docker CLI
├── desktop/      the app: Elyra (Rust + Svelte 5)
│   ├── src                  commands, streams, Kubernetes, the engine process
│   ├── app                  the Svelte frontend
│   ├── icons                the app icon
│   └── elyra.toml           the app's name, id, version, icon
├── scripts/bundle.sh   eDock.app, the .dmg, signing and notarization
└── docs/               this documentation
```

## Requirements

- An Apple silicon Mac with macOS 14+.
- Rust (stable, 1.94+), with Linux targets for the guest:

  ```sh
  rustup target add aarch64-unknown-linux-musl
  ```

- Node.js 20.19+ or 22.12+, for the frontend.
- A Docker engine, for building the kernel and the disk template. OrbStack, Docker Desktop, or eDock itself once it runs.
- The Elyra CLI, `rata`, from the [Elyra Framework](https://github.com/kwhorne/elyra-framework).

## The engine

```sh
cd engine
cargo xtask kernel    # guest/out/Image (in a container; about a minute, cached in the edock-kbuild volume)
cargo xtask disk      # guest/out/data.img (sparse 64 GiB ext4 template)
cargo xtask guest     # guest/out/initramfs.cpio (the guest agent)
cargo xtask host      # target/release/edock, signed with the virtualization entitlement
cargo xtask bundle    # target/edock-engine/{MacOS/edock, Resources/guest/…}
```

The kernel and disk need building only when their config changes; `guest` and `host` after each change.

Run it from the checkout. The binary finds `guest/out` itself:

```sh
EDOCK_NO_FINDER=1 EDOCK_HTTP_PORT=8081 EDOCK_HTTPS_PORT=8443 ./target/release/edock daemon
docker --host unix://$HOME/.edock/run/docker.sock run --rm alpine echo hello
```

`cargo xtask linux` builds the engine for a Linux host (no VM), in a container.

## The desktop app

```sh
cd desktop
(cd app && npm install && npm run build)
rata codegen          # app/src/bindings.ts from the Rust commands
cargo run
```

With `rata dev`, the frontend reloads as you edit it (Vite HMR).

The app finds the engine at `engine/target/release/edock` in the checkout, so **Start** on the Engine page runs your build.

### Adding a command

1. Write an `async fn` with `#[command]` in `desktop/src`. A fallible one returns `Result<T, String>`. Spell `Result` out in full: with a type alias, the macro can't see it's fallible, and the frontend gets `{Ok: …}`.
2. Add it to `commands![…]` in `core()` in `main.rs`.
3. Run `rata codegen`, and call it as `api.your_command(…)`.

### Gotchas

- **Build the engine with `cargo xtask host`, not `cargo build -p edock`.** Starting a VM needs the `com.apple.security.virtualization` entitlement, and only `xtask host` signs it in. An engine built with plain `cargo build` answers on its socket, but the first container fails with *The process doesn't have the "com.apple.security.virtualization" entitlement*.
- **Quit eDock before `rata codegen`.** The app is single-instance (it stays in the menu bar). While it runs, codegen's launch only brings the running app forward, and the bindings aren't written.
- **Restart the app after `npm run build`.** A page that's already open still names the old build's files.
- **The Elyra dependency.** `desktop/Cargo.toml` and `desktop/app/package.json` use Elyra's published `v0.46.0` release. To work on Elyra and eDock together, point both at a local checkout for a while: a `path` dependency, and `file:` packages. Don't commit that.

## Tests

### Engine

```sh
cd engine
cargo test                 # unit tests
SOAK=600 tests/e2e.sh      # ~350 checks through the docker CLI, then a leak soak (K8S=1 adds Kubernetes)
```

### Desktop

```sh
cd desktop
cargo test                 # unit tests: Kubernetes parsing, name checks
cargo test -- --ignored    # integration tests against docker's current context
EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock cargo test -- --ignored   # … against eDock
EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock EDOCK_TEST_K8S=1 cargo test kubernetes -- --ignored
```

The integration tests drive the app's commands through Elyra's `TestApp`, against a real engine. They make their own containers and volumes, and remove them afterwards.

- **The container test** covers the list, the Compose grouping, the log stream, a terminal (input, output, resize), stop and start, and a project's start.
- **The Kubernetes test** makes a cluster, deploys nginx, and covers:
  - the pod and service lists;
  - a pod's log and shell;
  - name checks;
  - restart and delete;
  - deleting the cluster at the end.

  It needs no cluster to be there already, and refuses to run if one is.

### Seeing the app

While a debug build runs, `rata mcp dev` lets a coding agent see its windows as accessibility snapshots and screenshots, and click and type in them. See Elyra's documentation.

## Keeping up with Kiri

The engine was forked from Kiri at `baa2d41`. `engine/UPSTREAM.md` lists every change made in the fork. To bring in a later Kiri fix:

1. Diff Kiri's change.
2. Apply it with the `kiri` → `edock` names.
3. Add it to `UPSTREAM.md`.
