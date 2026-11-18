# Containers

**Containers** is the app's first page. It lists every container on the current engine, running or not, and updates the moment one starts, stops or changes. The app follows the engine's events instead of polling.

## The list

Each row shows:

- **A status dot.** Green while running; amber while paused, restarting or unhealthy.
- **The name**, with the image and the short ID under it. On eDock's engine, a running container's **address** is under them: `blog.edock.localhost`, or `web.shop.edock.localhost` for a Compose service. Click it to open it. It reaches the container's web port whether or not that port is published. A container with no web page (a database, a worker with no ports) answers with a page that says what it listens on; the label `app.edock.http.port` picks the port when eDock guesses wrong.
- **Published ports**, as `8080→80`. Click one to open `http://localhost:8080` in your browser.
- **The state** (running, exited, created, …), its health when the image has a health check, and its status: how long it has been up, or for one that has stopped, its exit code and when (`Exited (3) 16 minutes ago`). The container's page shows the exit code too, in red when it isn't 0.
- **Actions**, which become clearer on hover:
  - **Terminal** and **Stop** while it runs; **Start** when it doesn't.
  - **Restart** and **Remove**.

Click a row, or press Enter on it, to open the container. Right-click a row for more:

- Open, Terminal and Logs
- for a Compose service, **Recreate** and **Rebuild & recreate** that service (see [Compose projects](#compose-projects))
- Start or Stop, Restart, and Pause or Resume
- Copy ID and Copy name
- Remove

The header shows how many containers are running, and how many there are in all. **Filter** narrows the list by container name, image or Compose project (projects that are down too).

**Prune** removes every stopped container, after asking. It never removes a Linux machine or the Kubernetes node, even stopped: they hold a whole system, and go only by their own **Delete**.

eDock's own containers are hidden from the list: Linux machines (`machine-<name>`), the Kubernetes node (`edock-k8s`) and BuildKit. They have pages of their own. **eDock's own** in the header shows them, marked as what they are.

## New service

**New service**, at the top of the list (and in ⌘K), starts a common service for development, ready to use, or an app server for a project of yours:

| Recipe | Image | Connect with |
|---|---|---|
| PostgreSQL | `postgres:17-alpine` | `postgres://postgres:postgres@localhost:5432/postgres` |
| MySQL | `mysql:8.4` | `mysql://root:mysql@localhost:3306/app` |
| MariaDB | `mariadb:11` | `mysql://root:mariadb@localhost:3306/app` |
| ElyraSQL | `ghcr.io/kwhorne/elyrasql:1.12` | `mysql://root:elyrasql@localhost:3307/elyra` |
| SQL Anywhere | `ghcr.io/kwhorne/sqlanywhere-server:0.11` | `http://localhost:8080` (no password) |
| Askr | `ghcr.io/kwhorne/askr:1.9` | `http://localhost:8080`, and `http://<folder>.edock.localhost` |
| Redis | `redis:8-alpine` | `redis://localhost:6379` |
| MongoDB | `mongo:8` | `mongodb://localhost:27017` |
| RabbitMQ | `rabbitmq:4-management-alpine` | `amqp://guest:guest@localhost:5672`, UI on 15672 |
| Mailpit | `axllent/mailpit` | SMTP on `localhost:1025`, UI on 8025 |
| MinIO | `minio/minio` | `http://localhost:9000` (user `minio`, password `minio123`), console on 9001 |

Pick one, give it a name if you like (the recipe's short name otherwise, such as `postgres`, or `-2`, `-3`… when that's taken), and press **Start**. eDock pulls the image if needed, keeps the data in a volume `<name>-data` (Mailpit and Askr keep none), publishes the port on your Mac, on `127.0.0.1` only (the usual port, or the next free one when it's taken), and restarts the service with the engine. When it's up, the connection string is copied, and the note shows it. A service with a web UI shows its address in the note, and on eDock's engine `http://<name>.edock.localhost` reaches it too.

[ElyraSQL](https://github.com/kwhorne/ElyraSQL) speaks MySQL's protocol, so any MySQL client, driver or framework connects to it as to MySQL (Laravel with `DB_CONNECTION=mysql`, `DB_PORT=3307`, `DB_DATABASE=elyra`). It keeps the whole database in one file in its volume, and has one database, `elyra`: `CREATE DATABASE` isn't needed.

[SQL Anywhere](https://github.com/kwhorne/sql-anywhere) runs `sqld`: SQLite over HTTP and WebSockets (the Hrana protocol), with vector search. Its clients and the `elyra` CLI connect to the URL (`elyra db shell http://127.0.0.1:8080`); `GET /health` says it's up. Its database lives in the volume, as an ordinary SQLite file.

[Askr](https://github.com/kwhorne/askr) is a PHP 8.5 application server, so it serves **a folder from your Mac** rather than keeping data of its own: **Choose folder…** picks the project (under `/Users` or `/Volumes`), and the container is named after it. A Laravel project (it has `artisan`) runs in worker mode, booted once and served many times; any other PHP project is served from its `public/` folder, or from the folder itself. It runs as your Mac user, so Laravel can write `storage/` and `bootstrap/cache`, and the files it writes are yours. Askr doesn't build the project: run `composer install` first. Inside it, `127.0.0.1` is the container itself: a database published on your Mac (MySQL, ElyraSQL, PostgreSQL from **New service**) is `DB_HOST=host.docker.internal` in the project's `.env`. From ⌘K, **New service: Askr** asks for the folder.

The passwords are well known, so the ports are open to this Mac only, not to your network. Other containers reach the service by its name on a shared network, or at `host.docker.internal:<port>`.

## Run image

**Run image…**, at the top of the list (also **Run…** on an image, and **Run image…** in ⌘K), starts a container from any image, as `docker run -d` does:

- **Image**: pulled first when it isn't here.
- **Name**: the image's name when left empty (`nginx`, then `nginx-2`…). A name that's taken is said, not changed.
- **Ports**: a port on your Mac → a port in the container. The ports the image exposes are filled in, each on a free port of your Mac (a port under 1024 goes from 8000 up: 80 → 8080). Leave the Mac's empty for any free one. They're open to this Mac only, unless **Open to other devices on the network** is on.
- **Folders and volumes**: a folder from your Mac (**Mac folder…**, under `/Users` or `/Volumes`) or a volume by name (made when it isn't there) → a path in the container, read-only if you like. Where the image keeps data (its `VOLUME`s), a **+ volume at …** button puts a volume there.
- **Environment**: a variable per row. Typing `KEY=value` splits it; pasting a `.env` adds a row for each line.
- **Command**: in place of the image's own (shown greyed out), split as a shell splits it (quotes work, nothing is expanded).
- **Start it again when the engine starts**: `--restart unless-stopped`.

When it runs, its page opens. A container that doesn't start is removed again and the error is shown, so you can change something and press **Run** again.

## Compose projects

Containers that Docker Compose made are grouped under their project, sorted by name. Within a project, each container shows its **service name**.

A project's header shows:

- **A status dot:** green when all of its containers run, amber when only some do.
- **How many run**, for example `2/3 running`.
- **The project's folder**, from Compose's `working_dir` label. Click it to open it in **Finder**, **Terminal** or your editor (Cursor, VS Code, Zed, Windsurf, Sublime Text or Nova, whichever is installed), or to copy it.
- **Actions for the whole project:**
  - **What it's made of** (the page icon): each service as `docker compose config` reads the project's files, those in a profile too, with its image or build folder, ports, volumes, the services it needs, its profiles, and the names of its variables. The project's `.env` (or the env files it was started with) shows its variables' names too. Values never show: they're often passwords. Each service has **Recreate** (`docker compose up -d --no-deps --force-recreate SERVICE`), and one that's built has **Rebuild & recreate** (`… --build`), which make that service again and leave the rest as they are. Use them after changing its part of `compose.yaml` or `.env`, or its code.
  - **All its logs**: every service's log as one, in time order, each line with its service in its own colour. Click a service's name above the log to hide it; filter, timestamps and copy work as for one container.
  - **Rebuild & up**: `docker compose up -d --build --remove-orphans` (a service you took out of the Compose file is removed), from the project's own Compose files and folder (from the containers' labels), on the engine the app shows. Its output shows as it runs. It needs the `docker` command with Compose: [Set up](engine-and-contexts.md#command-line) installs them.
  - **Start all**, which skips the containers already running.
  - **Stop all**, which skips the stopped ones.
  - **Restart all**.
  - **Remove all**, which keeps the volumes, and the project (see below).

The arrow folds a project away.

### Projects that are down

eDock remembers each Compose project it sees on an engine: its folder, its Compose files and any `--env-file`, from the containers' labels. A project whose containers are gone (`docker compose down` in a terminal, or **Remove all**) stays in the list, marked **down**, with:

- **Up**: `docker compose up -d --remove-orphans` from its own files and folder, on the engine the app shows. Its output shows as it runs.
- **Rebuild & up**, as above.
- **Forget**, which takes it off the list. Its folder, files and volumes stay.

When its folder or one of its files is gone, it says so, and Up waits until it's back.

**Open Compose project…** at the top adds a project that hasn't run on this engine yet: pick its folder. eDock finds the Compose file there as `docker compose` does (`compose.yaml`, `compose.yml`, `docker-compose.yml` or `docker-compose.yaml`, with an override file beside it) and asks `docker compose` for the project's name. The project shows as down until you press **Up**.

Projects are remembered for each docker context, in `~/.edock/desktop.json`.

### On demand

On eDock's engine, a Compose project (or a container outside Compose) can be **on demand**: it stops when nothing has used it for a while, and starts again on the next connection to one of its ports or its name (`NAME.edock.localhost`). That connection waits for it, usually a few hundred milliseconds, a little more when the VM was asleep too. While it's stopped, it's **parked**: it uses no memory, nothing else can take its ports, and when everything is parked the VM sleeps.

The moon button on a project's row (or a container's right-click menu) turns it on: **Park after 5 minutes unused**, **15 minutes** or **an hour**, or **Keep running**. A project with the label `app.edock.on-demand` on one of its services (`"15m"`) is on demand without this; **As its label says** goes back to it after you chose otherwise. **Park now** parks it at once.

- An on-demand project shows the moon and its time beside its name, and **parked** while it's parked, on its row and on each of its containers. **Wake** (the play button) starts it.
- All of a project parks and wakes as one, its services in their `depends_on` order, waiting for `service_healthy` where it says so.
- Unused means no connection open through its ports or name, nothing sent by its containers (to each other too), and no terminal or `docker exec -it` in one. A database client that keeps a connection open keeps it awake.
- A service without a published port, such as a queue worker or a scheduled job, doesn't run while its project is parked. What a container keeps only in memory is lost when it parks, as with `docker stop`. eDock says so when you turn it on.
- A container that exits as it wakes stays parked; a page at its name says why, and the next connection tries again. **Stop** makes a parked one plainly stopped.
- [Requests](requests.md) marks the request that woke one, and how long it waited. The same settings are on the command line as [`edock on-demand`](../cli.md#edock-on-demand), and kept in `~/.edock/on-demand.json`.

Containers outside Compose come after the projects, the running ones first.

## One container

The container's page shows, at the top:

- the image;
- the command (entrypoint and arguments);
- the published ports, as `host:port → container port`;
- its networks, with its address on each.

It has **Stop/Start**, **Restart**, **Snapshot** and **Remove** buttons, and six tabs. **Snapshot** keeps the container's files as they are now; see [Snapshots](snapshots.md).

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

- **Links**: a web address in the output is a link. **⌘-click** it to open it in your browser; a plain click still selects text. Over a link, the bottom corner says where it goes.
- **Find**: **⌘F** opens a find bar over the terminal, with what's selected in it already. It finds in what the terminal shows and the 5,000 lines it has scrolled by, and says how many matches there are. **Enter** or **⌘G** goes to the next one, **⇧Enter** or **⇧⌘G** to the one before, and **Esc** closes it.

The same terminal is in [Linux machines](machines.md) and [Kubernetes pods](kubernetes.md#a-pod) (**Shell**).

### Files

Browse the container's folders, starting at `/`. Click a folder to open it, and the path at the top to go back up. Folders come first, each entry with its permissions, size and age.

Click a file to preview it beside the list: text shows (its first 256 KB), and a link shows where it points, with **Go there**.

| | |
|---|---|
| **Save to the Mac** | Saves the file where you choose. A folder is saved as a `.tar`. |
| **Put files here** | Copies files or folders from the Mac into the folder you're in. You can also drop them on the window. They're owned by root. |
| **Remove** (bin) | Removes the file or folder, after asking. Not where a Mac folder or volume is mounted, nor a folder with a mount under it: that would empty what's behind it. |
| **Show in Finder** | On eDock's engine: the file in `~/eDock`, eDock's read-only Finder view of running containers |

The tab needs the container to be running; when it stops, the tab says so. On eDock's engine browsing works with any image, even one without a shell. On other engines it runs `find` and `stat` in the container, so an image without them can't be browsed, and the tab says so. Removing runs `rm` in the container on every engine, so it needs an image with `rm`.

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

- **Environment:** every variable, sorted. On eDock's engine, **Edit** changes them, one row per variable. A value is kept exactly as you type it, over several lines too (a PEM key, JSON), with any spaces at its ends. A running container restarts with the new environment, and its files are kept.
- **Mounts:** each one's path in the container, its kind (volume or bind) and source, and whether it's read-only.
- **Labels.**
- **The full `docker inspect`** as JSON, with a copy button.

## Keyboard

- **⌘K** opens the command palette. Type `Containers`, `Images`, `Kubernetes` and so on to go there. It also finds things by name: containers (by name, image, Compose project or short ID), images, Linux machines and Kubernetes pods. Pick one to open it. It has **Run image…**, **Check for updates**, and **Start eDock engine** while eDock's engine isn't running.
- In ⌘K, **New service: …** (after the containers it finds) starts a service from a [recipe](#new-service).
- **Tab** moves through the list; **Enter** opens a container.
