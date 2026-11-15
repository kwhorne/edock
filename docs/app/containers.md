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

The header shows how many containers are running, and how many there are in all. **Filter** narrows the list by container name, image or Compose project.

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

Pick one, give it a name if you like (the recipe's name otherwise, or `-2`, `-3`… when that's taken), and press **Start**. eDock pulls the image if needed, keeps the data in a volume `<name>-data`, publishes the port on your Mac, on `127.0.0.1` only (the usual port, or the next free one when it's taken), and restarts the service with the engine. When it's up, the connection string is copied, and the note shows it. A service with a web UI also opens it at `http://<name>.edock.localhost`.

[ElyraSQL](https://github.com/kwhorne/ElyraSQL) speaks MySQL's protocol, so any MySQL client, driver or framework connects to it as to MySQL (Laravel with `DB_CONNECTION=mysql`, `DB_PORT=3307`, `DB_DATABASE=elyra`). It keeps the whole database in one file in its volume, and has one database, `elyra`: `CREATE DATABASE` isn't needed.

[SQL Anywhere](https://github.com/kwhorne/sql-anywhere) runs `sqld`: SQLite over HTTP and WebSockets (the Hrana protocol), with vector search. Its clients and the `elyra` CLI connect to the URL (`elyra db shell http://127.0.0.1:8080`); `GET /health` says it's up. Its database lives in the volume, as an ordinary SQLite file.

[Askr](https://github.com/kwhorne/askr) is a PHP 8.5 application server, so it serves **a folder from your Mac** rather than keeping data of its own: **Choose folder…** picks the project (under `/Users` or `/Volumes`), and the container is named after it. A Laravel project (it has `artisan`) runs in worker mode, booted once and served many times; any other PHP project is served from its `public/` folder, or from the folder itself. It runs as your Mac user, so Laravel can write `storage/` and `bootstrap/cache`, and the files it writes are yours. Askr doesn't build the project: run `composer install` first. Inside it, `127.0.0.1` is the container itself: a database published on your Mac (MySQL, ElyraSQL, PostgreSQL from **New service**) is `DB_HOST=host.docker.internal` in the project's `.env`. From ⌘K, **New service: Askr** asks for the folder.

The passwords are well known, so the ports are open to this Mac only, not to your network. Other containers reach the service by its name on a shared network, or at `host.docker.internal:<port>`.

## Compose projects

Containers that Docker Compose made are grouped under their project, sorted by name. Within a project, each container shows its **service name**.

A project's header shows:

- **A status dot:** green when all of its containers run, amber when only some do.
- **How many run**, for example `2/3 running`.
- **The project's folder**, from Compose's `working_dir` label. Click it to open it in **Finder**, **Terminal** or your editor (Cursor, VS Code, Zed, Windsurf, Sublime Text or Nova, whichever is installed), or to copy it.
- **Actions for the whole project:**
  - **All its logs**: every service's log as one, in time order, each line with its service in its own colour. Click a service's name above the log to hide it; filter, timestamps and copy work as for one container.
  - **Rebuild & up**: `docker compose up -d --build`, from the project's own Compose files and folder (from the containers' labels), on the engine the app shows. Its output shows as it runs. It needs the `docker` command with Compose: [Set up](engine-and-contexts.md#command-line) installs them.
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

### Files

Browse the container's folders, starting at `/`. Click a folder to open it, and the path at the top to go back up. Folders come first, each entry with its permissions, size and age.

Click a file to preview it beside the list: text shows (its first 256 KB), and a link shows where it points, with **Go there**.

| | |
|---|---|
| **Save to the Mac** | Saves the file where you choose. A folder is saved as a `.tar`. |
| **Put files here** | Copies files or folders from the Mac into the folder you're in. You can also drop them on the window. They're owned by root, as with `docker cp`. |
| **Remove** (bin) | Removes the file or folder, after asking |
| **Show in Finder** | On eDock's engine: the file in `~/eDock`, eDock's read-only Finder view of running containers |

Browsing needs the container to be running. On eDock's engine it works with any image, even one without a shell. On other engines it runs `find` and `stat` in the container, so an image without them can't be browsed. Previewing and saving work on a stopped container too, on any engine.

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

- **⌘K** opens the command palette. Type `Containers`, `Images`, `Kubernetes` and so on to go there. It also finds things by name: containers (by name, image or Compose project), images, Linux machines and Kubernetes pods. Pick one to open it.
- In ⌘K, **New service: …** starts a service from a [recipe](#new-service).
- **Tab** moves through the list; **Enter** opens a container.
