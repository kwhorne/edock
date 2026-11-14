# Upstream

`engine/` is a fork of Kiri — https://github.com/libracoder/kiri (Apache-2.0,
Copyright Canestack Labs and the Kiri contributors).

Forked from commit `baa2d41c1ce660b7ee255efc4fec156af42a715d` (2026-11-14).
The desktop UI is the eDock app in `../desktop`; it talks to this engine only
through the Docker Engine API (plus the engine's own `/edock/...` endpoints)
and runs `edock daemon` / `edock setup`.

## Changes in eDock

### Removed

- The webview UI: `crates/kiri/src/ui/` (macOS WKWebView and Linux GTK
  windows, xterm, translations), the `ui` subcommand, the `window` cargo
  feature and the gtk/webkit2gtk/javascriptcore and objc2-app-kit/web-kit/
  core-text/core-foundation dependencies; `tests/ui`, `scripts/ui-test.py`,
  `scripts/ui-icons.py`, `scripts/i18n-check.py`, the UI-tests workflow.
- Kiri Share (`crates/kiri/src/share.rs`, `crates/kiri-share-proto`, the build
  key script `build_share_key.rs`/`build.rs`): it talked to runkiri.cloud.
  The `share` subcommand and the `kiri PORT` shorthand are gone.
- The self-updater (`crates/kiri/src/update.rs`, `kiri update`), which read
  the download.runkiri.com feed.
- Crash/report upload to Kiri Cloud: `diag report --send` now fails with "not
  available"; the daemon's background crash sender, install id and upload
  code are gone. `diag report` still writes a local, redacted file.
- Running the app bundle with no arguments now only does first-launch setup
  (it used to open the window).
- Windows: `crates/kiri-win` (dropped from the workspace), `installer/windows`,
  `contrib/windows`, `contrib/wsl`, the `windows`/`windows-dev`/`msi`/
  `msi-pack`/`docker-win-arm64` xtasks, `scripts/windows-engine.sh`,
  `scripts/kiri-linux.sh`, `tests/wsl.sh`, `docs/testing-windows-arm.md`.
  The Linux-side halves that only kiri-win used (`bridge.rs`, `hostrelay.rs`,
  `wsl.rs`, Windows paths in `daemon/binds.rs` and `daemon/proxy.rs`) are
  still in the tree, compiled only for Linux/Windows, and their comments
  still mention `edock-win`.
- Distribution of Kiri itself: the `app`, `app-lite`, `app-dev`, `dmg`,
  `release`, `sign`, `pack` xtasks (Kiri.app, DMGs, update feeds,
  notarization) and `xtask/dev-icon.swift`; `packaging/` (deb/rpm/Homebrew),
  `contrib/linux` desktop entry/metainfo/deb script, `scripts/dmg-*`,
  `scripts/ci/test-version.sh`, `scripts/ci/linux-packages.sh`,
  `scripts/ci/linux-build.sh`.
- Docs and brand: `brand/`, `docs/releases`, `docs/images`, `RELEASES.md`,
  `CLA.md`, Kiri's benchmark results (`bench/results.json`, `bench/*-2026-*.md`,
  `bench/compare.ps1`), `.github/ISSUE_TEMPLATE/config.yml`. README.md was
  replaced; `docs/development.md`, `docs/features.md`, `CONTRIBUTING.md`,
  `SECURITY.md`, `guest/kernel/SOURCE_OFFER.md` were rewritten for eDock.

### Added

- `cargo xtask bundle`: the signed binary and the guest laid out as an app
  bundle's `Contents` (`target/edock-engine/MacOS/edock`,
  `target/edock-engine/Resources/guest/{Image,initramfs.cpio,data.img.tgz}`,
  LICENSE, NOTICE, kernel source offer), for the desktop app to copy in.

### Renamed (scripted, then reviewed)

- Crates `kiri`, `kiri-image`, `kiri-init`, `kiri-proto`, `kiri-vmm` →
  `edock`, `edock-image`, `edock-init`, `edock-proto`, `edock-vmm`
  (`kiri_proto` → `edock_proto` etc.); binary `kiri` → `edock`;
  `kiri.entitlements` → `edock.entitlements`.
- `Kiri` → `eDock` in text; `Kiri`-prefixed identifiers → `Edock…`
  (`EdockVmDelegate`; Docker API extension fields such as `EdockMemoryUsed` in `/info` and `EdockExitCode`), `X-Kiri-*`
  headers → `X-Edock-*`, `/kiri/...` API routes → `/edock/...`.
- `KIRI_*` env vars → `EDOCK_*`; `~/.kiri` → `~/.edock`;
  `~/Library/Application Support/Kiri` → `.../eDock`; Finder mount `~/Kiri`
  → `~/eDock`; `*.kiri.local`/`*.kiri.localhost` → `*.edock.local`/
  `*.edock.localhost`; `host.kiri.internal` → `host.edock.internal`.
- Docker/kubectl context and buildx builder `kiri`/`kiri-build` →
  `edock`/`edock-build`; bundle, LaunchAgent/LaunchDaemon and label
  identifiers `dev.kiri.*` → `app.edock.*` (`app.edock.app`,
  `app.edock.daemon`, `app.edock.docker-socket`, container labels
  `app.edock.domain` etc.); Kiri Dev → eDock Dev (`app.edock.app.dev`).
- Guest: virtiofs tag `kiri-mac` → `edock-mac`, kernel config
  `guest/kernel/kiri*.config` → `edock*.config` (kernel reports
  `6.18-edock`), data disk label `edock-data`, kernel build cache volume
  `edock-kbuild`.

### Notes

- Some functions only the removed window called are kept (marked
  `#[allow(dead_code)]` at the module) for the desktop app to reuse; the
  engine's `/edock/...` HTTP endpoints (resources, proxy, domains, USB,
  builder, image scans, k8s forwards) are all still served.
- Error messages still point at "Settings → …" screens the Kiri window had;
  the eDock desktop is expected to offer equivalents.
- `NOTICE` is upstream's file unchanged below the prepended attribution, so
  its copied kernel source offer still names Kiri's download site.

- Container-domain routers bind both IPv4 and IPv6 or neither: Kiri took a port on IPv6 alone when another server held it on IPv4 (a local web server on 80/443), so `localhost` (::1 first) reached eDock instead of it.
