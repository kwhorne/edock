# eDock

A tiny, fast container runtime for the desktop: its own Linux VM and Docker
Engine API (forked from [Kiri](https://github.com/libracoder/kiri), Apache-2.0),
with a desktop app built on the [Elyra Framework](../elyra-framework).

```
docker CLI ─┐
compose ────┼─ ~/.edock/run/docker.sock ─► edock daemon ── vsock ──► guest agent (PID 1)
eDock app ──┘     Docker Engine API          owns the VM            clone3 → container
```

| Path | What |
|------|------|
| `engine/` | The engine: `edock` daemon + CLI, VM, guest agent, kernel config. Fork of Kiri; see `engine/UPSTREAM.md` and `engine/NOTICE`. |
| `desktop/` | The desktop app (Elyra: Rust + Svelte 5): containers grouped by Compose project, images, volumes, networks, live logs, stats, a terminal, docker contexts, and a menu bar item. |

## Build

```sh
# Engine (macOS, Apple silicon; needs a Docker engine for the kernel build)
cd engine
rustup target add aarch64-unknown-linux-musl
cargo xtask kernel && cargo xtask disk && cargo xtask guest && cargo xtask host
#   → target/release/edock (signed with the virtualization entitlement)
cargo xtask bundle   # target/edock-engine/{MacOS/edock,Resources/guest/…}

# Desktop
cd ../desktop
(cd app && npm install && npm run build)
rata codegen         # quit a running eDock first: it's single-instance
cargo run
```

### eDock.app and a release

```sh
scripts/bundle.sh                         # eDock.app, ad hoc (this Mac only)
scripts/bundle.sh --dmg                   # … and dist/eDock-<version>-arm64.dmg
scripts/bundle.sh --skip-engine           # reuse engine/target/edock-engine

# A release: Developer ID, notarized and stapled
xcrun notarytool store-credentials edock-notary --apple-id … --team-id …   # once
EDOCK_SIGN_ID="Developer ID Application: … (TEAMID)" EDOCK_NOTARY_PROFILE=edock-notary \
  scripts/bundle.sh --version 0.1.0 --notarize
```

`--version` writes one version into `desktop/elyra.toml`, `desktop/Cargo.toml`
and `engine/Cargo.toml` (the app, its About box and the engine's
`/version`); without it the script refuses to build when they differ.
For CI, `EDOCK_NOTARY_KEY`/`EDOCK_NOTARY_KEY_ID`/`EDOCK_NOTARY_ISSUER` (an App
Store Connect API key) replace the keychain profile.

`rata bundle` makes the app; the script then puts the engine in
(`Contents/MacOS/edock`, `Contents/Resources/guest`), signs it with the
virtualization entitlement (`rata bundle`'s `--deep` signing would drop it),
seals the app again, and with `--notarize` notarizes and staples the app,
then the `.dmg` around it (so both open offline), and writes a `.sha256`.

The app finds the engine at `$EDOCK_ENGINE`, beside its own binary,
`~/.edock/bin/edock`, or `engine/target/release/edock` in this checkout. It
talks to any engine through the Docker Engine API, so it also manages OrbStack,
Docker Desktop, Colima or Podman (switch in the toolbar or on *Engine*).

## Test

```sh
cd engine && cargo test
cd desktop && cargo test -- --ignored                       # against docker's current context
EDOCK_TEST_HOST=unix://$HOME/.edock/run/docker.sock cargo test -- --ignored   # against eDock
```

When trying the engine, `EDOCK_NO_FINDER=1` skips the `~/eDock` Finder
mount, and `EDOCK_HTTP_PORT` / `EDOCK_HTTPS_PORT` move the `*.edock.localhost`
routers off ports 80/443.

## License

The engine is Apache-2.0 (Kiri's licence; keep `engine/LICENSE` and
`engine/NOTICE`). The guest kernel is GPL-2.0.
