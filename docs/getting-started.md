# Getting started

## Requirements

- A Mac with **Apple silicon** (M1 or later). There is no Intel version.
- **macOS 14** (Sonoma) or later.
- Nothing else. eDock brings the `docker` CLI, Compose and buildx when you set up the command line (below). No package manager needed.

## Install

1. Download `eDock-<version>-arm64.dmg` from [elyracode.com/edock](https://elyracode.com/edock), or from the [releases on GitHub](https://github.com/kwhorne/edock/releases).
2. Optionally, check it: `shasum -a 256 -c eDock-<version>-arm64.dmg.sha256`.
3. Open the dmg and drag **eDock** into **Applications**.

The app is signed with a Developer ID and notarized by Apple, so it opens without a warning, even offline.

## First launch

Open eDock from Applications, or with Spotlight. eDock lives in the **menu bar**. It has no Dock icon while its window is closed. The main window opens on the first launch.

The first time, a **welcome** page walks through the rest of this guide in four clicks: start the engine, set up the terminal, bring things over from OrbStack or Docker Desktop if you have it, and trust HTTPS. Each step can wait; **Skip** leaves them all for the Engine page. ⌘K › *Welcome* shows it again.

The app first connects to whatever engine your docker setup uses: eDock's own when it runs, otherwise docker's current context (OrbStack or Docker Desktop, say). The toolbar shows which one, and you can switch at any time. See [Engine and contexts](app/engine-and-contexts.md).

## Start the eDock engine

Open **Engine** at the bottom of the sidebar and press **Start**. Or choose **Start eDock engine** in the menu bar item's menu.

The engine's socket is up at once, and the app switches to it. The Linux VM boots the first time something needs it, in about 0.15 seconds. After 60 seconds with nothing to do it goes to sleep, and the engine itself then uses about 15 MB of memory.

When the engine starts, it also:

- serves containers at `http://<name>.edock.localhost` on ports 80 and 443;
- mounts container files at `~/eDock` in Finder (read-only).

[Configuration](configuration.md#environment-variables) shows how to move or turn these off.

## Use it from the terminal

On the **Engine** page, under **Command line**, press **Set up**. Leave *Make `edock` docker's current context* ticked to have `docker` use eDock straight away.

This sets up:

- **The `docker` CLI, Docker Compose and buildx.** They're downloaded once, the versions eDock is tested with, and each one is checked against its published SHA-256 before it's used. They're kept in `~/Library/Application Support/eDock/cli`.
- **`docker` and `edock` in `~/.edock/bin`,** added to the end of your PATH, so a `docker` you already have keeps priority.
- **An `edock` docker context.**
- **A LaunchAgent** that starts the engine when you log in.
- **An `edock-build` buildx builder.**

Open a new terminal window afterwards, so it picks up the PATH. The same from a terminal:

```sh
/Applications/eDock.app/Contents/MacOS/edock setup --use
```

[Using docker with eDock](docker.md#edock-setup) lists everything it does, and how to undo it.

Then:

```sh
docker run --rm alpine echo hello
docker compose up -d
```

To try eDock without changing anything, name its socket instead:

```sh
docker --host unix://$HOME/.edock/run/docker.sock run --rm alpine echo hello
```

## Your first container in the app

1. Run something from the terminal, for example `docker run -d -p 8080:80 --name web nginx:alpine`.
2. It appears in **Containers** at once, with its port. Click **8080** to open it in the browser.
3. Click the container to see its log, live stats, a terminal and `docker inspect`.

## Next

- [Containers](app/containers.md): everything the container list does.
- [Kubernetes](app/kubernetes.md): a cluster in one click.
- [Updates](app/updates.md): how eDock keeps itself current.
- [Troubleshooting](troubleshooting.md): if something doesn't work.
