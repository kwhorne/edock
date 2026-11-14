# Containers

**Containers** is the app's first page. It lists every container on the current engine, running or not, and updates the moment one starts, stops or changes. The app follows the engine's events instead of polling.

## The list

Each row shows:

- **A status dot.** Green while running; amber while paused, restarting or unhealthy.
- **The name**, with the image and the short ID under it. On eDock's engine, a running container's **address** is under them: `blog.edock.localhost`, or `web.shop.edock.localhost` for a Compose service. Click it to open it. It reaches the container's web port whether or not that port is published.
- **Published ports**, as `8080→80`. Click one to open `http://localhost:8080` in your browser.
- **The state** (running, exited, created, …), its health when the image has a health check, and how long it has been up.
- **Actions**, which become clearer on hover:
  - **Terminal** and **Stop** while it runs; **Start** when it doesn't.
  - **Restart** and **Remove**.

Click a row, or press Enter on it, to open the container. Right-click a row for more:

- Open, Terminal and Logs
- Start or Stop, Restart, and Pause or Resume
- Copy ID and Copy name
- Remove

The header shows how many containers are running, and how many there are in all. **Filter** narrows the list by container name, image or Compose project. **Prune** removes every stopped container, after asking.

## Compose projects

Containers that Docker Compose made are grouped under their project, sorted by name. Within a project, each container shows its **service name**.

A project's header shows:

- **A status dot:** green when all of its containers run, amber when only some do.
- **How many run**, for example `2/3 running`.
- **The project's folder**, from Compose's `working_dir` label.
- **Actions for the whole project:**
  - **Start all**, which skips the containers already running.
  - **Stop all**, which skips the stopped ones.
  - **Restart all**.
  - **Remove all**, which keeps the volumes.

The arrow folds a project away.

Containers outside Compose come after the projects, the running ones first.

## One container

The container's page shows, at the top:

- the image;
- the command (entrypoint and arguments);
- the published ports, as `host:port → container port`;
- its networks, with its address on each.

It has **Stop/Start**, **Restart** and **Remove** buttons, and five tabs.

### Logs

The log follows live, starting from the last 500 lines.

| Control | What it does |
|---|---|
| **Filter lines** | Shows only the lines containing the text |
| **Timestamps** | Shows Docker's timestamp on each line (the log restarts from the last 500 lines) |
| **Wrap** | Wraps long lines instead of scrolling sideways |
| **Copy** | Copies the lines shown, without colour codes |
| **Clear** | Empties the view; new lines keep coming |

Lines the container wrote to **stderr** have an amber mark on their left. The view keeps the newest 5,000 lines.

It scrolls with new lines while you're at the bottom. Scroll up to read, and it stays put; **↓ Follow** takes you back down.

### Stats

Live, about once a second, with a graph of the last minute for each:

- **CPU:** percent of one CPU, so 200% means two CPUs are busy. It also shows how many CPUs the container sees.
- **Memory:** used, less the page cache (as `docker stats` counts it), out of its limit.
- **Network:** received and sent per second, and the totals.
- **Disk:** read and written per second, the totals, and the number of processes.

### Terminal

This is a shell in the running container: `bash` when the image has it, otherwise `sh`. It's a full terminal with colours, cursor keys and full-screen programs such as `vim` and `top`. It follows the window's size.

When the shell exits, press **Enter** to open a new one.

### Ports

On eDock's engine, ports can be changed **while the container runs**, without making it again.

- **Address:** the container's name at eDock's router, as a link.
- **Listening, but not published:** eDock looks at what the container really listens on. A port it listens on that nothing publishes shows here, with **Publish** next to it. This happens when you forgot `-p`, or the image never declared the port. Publishing makes it `localhost:<same port>`.
- **Published:** each port on the Mac and the port in the container it reaches. A badge says whether the container listens there. **Remove** stops publishing it.
- **Publish a port:**
  - **Container port** is the port in the container.
  - **Mac port** is the port on the Mac. Leave it empty for the same number, or use 0 to let eDock pick a free one.
  - Choose TCP or UDP.
  - **This Mac only** binds to `127.0.0.1`, out of reach from your network.

Ports the container binds only on its own loopback are listed at the bottom: nothing outside the container can reach those.

### Inspect

- **Environment:** every variable, sorted. On eDock's engine, **Edit** changes them: one `NAME=value` per line. A running container restarts with the new environment, and its files are kept.
- **Mounts:** each one's path in the container, its kind (volume or bind) and source, and whether it's read-only.
- **Labels.**
- **The full `docker inspect`** as JSON, with a copy button.

## Keyboard

- **⌘K** opens the command palette. Type `Containers`, `Images`, `Kubernetes` and so on to go there.
- **Tab** moves through the list; **Enter** opens a container.
