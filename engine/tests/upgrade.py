#!/usr/bin/env python3
"""Upgrades: each released eDock's engine makes some state (containers
running and stopped, restart policies, a volume with files, a network, a
Compose project, a committed image, a parked project where it has On
demand), then this checkout's engine starts on the same data and must
bring all of it back.

    tests/upgrade.py                    # 0.2.0, 0.4.0, 0.6.5, 0.8.1 and 0.9.0
    tests/upgrade.py 0.7.0 0.9.0        # those
    tests/upgrade.py --new PATH         # another new engine (default: target/release/edock)

Each engine runs with a HOME of its own under $TMPDIR, so its socket,
settings and data are all there: nothing of the installed eDock is touched,
and its routers get ports of their own. Releases are fetched with `gh` and
kept in $TMPDIR/edock-upgrade-cache. Exits 1 when a check fails.
"""

import argparse
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TMP = tempfile.gettempdir()
CACHE = os.path.join(TMP, "edock-upgrade-cache")
DEFAULT = ["0.2.0", "0.4.0", "0.6.5", "0.8.1", "0.9.0"]
WEB_PORT, ROUTER_HTTP, ROUTER_HTTPS = 18591, 18580, 18543

failures = []


def say(*a):
    print(*a, flush=True)


def check(what, ok, got=""):
    say(f"    {'ok  ' if ok else 'FAIL'} {what}" + ("" if ok else f": {got}"))
    if not ok:
        failures.append(what)


def release(version):
    """The released app's folder, fetched once."""
    app = os.path.join(CACHE, version, "eDock.app")
    if os.path.isdir(app):
        return app
    dest = os.path.join(CACHE, version)
    os.makedirs(dest, exist_ok=True)
    subprocess.run(["gh", "release", "download", f"v{version}", "--repo", "kwhorne/edock", "-p", f"eDock-{version}-arm64.zip", "-D", dest, "--clobber"], check=True)
    subprocess.run(["ditto", "-x", "-k", os.path.join(dest, f"eDock-{version}-arm64.zip"), dest], check=True)
    return app


class Engine:
    """One engine with its own HOME (socket, settings, data)."""

    def __init__(self, binary, home, label):
        self.binary, self.home, self.label = binary, home, label
        self.sock = os.path.join(home, ".edock", "run", "docker.sock")
        self.proc = None

    def env(self):
        e = dict(os.environ, HOME=self.home, EDOCK_NO_FINDER="1", EDOCK_HTTP_PORT=str(ROUTER_HTTP), EDOCK_HTTPS_PORT=str(ROUTER_HTTPS))
        for k in ("DOCKER_HOST", "DOCKER_CONTEXT", "EDOCK_SOCKET", "EDOCK_DATA_DIR"):
            e.pop(k, None)
        return e

    def start(self):
        os.makedirs(os.path.join(self.home, ".edock"), exist_ok=True)
        self.log_from = os.path.getsize(self.log_path()) if os.path.exists(self.log_path()) else 0
        self.log = open(os.path.join(self.home, f"{self.label}.out"), "w")
        self.proc = subprocess.Popen([self.binary, "daemon"], env=self.env(), stdout=self.log, stderr=self.log, start_new_session=True)
        for _ in range(300):
            if self.ping():
                return
            if self.proc.poll() is not None:
                break
            time.sleep(0.1)
        raise SystemExit(f"upgrade: the {self.label} engine didn't come up (see {self.log.name})")

    def ping(self):
        try:
            s = socket.socket(socket.AF_UNIX)
            s.connect(self.sock)
            s.sendall(b"GET /_ping HTTP/1.0\r\n\r\n")
            ok = b"OK" in s.recv(256)
            s.close()
            return ok
        except OSError:
            return False

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=60)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.proc = None

    def sh(self, script, timeout=300):
        """A shell script with `$D` as docker on this engine; its output."""
        cfg = os.path.join(self.home, ".docker")
        os.makedirs(cfg, exist_ok=True)
        plugins = os.path.join(cfg, "cli-plugins")
        if not os.path.exists(plugins):
            os.symlink(os.path.expanduser("~/.docker/cli-plugins"), plugins)
        env = dict(os.environ, DOCKER_HOST=f"unix://{self.sock}", DOCKER_CONFIG=cfg, D="docker")
        env.pop("DOCKER_CONTEXT", None)
        r = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True, timeout=timeout)
        return (r.stdout + r.stderr).strip()

    def api(self, method, path, body=None):
        """One request to the engine's API: (status, body). The engine keeps
        a connection open, so the body is read by its length."""
        data = json.dumps(body).encode() if body is not None else b""
        s = socket.socket(socket.AF_UNIX)
        s.settimeout(60)
        s.connect(self.sock)
        s.sendall(f"{method} {path} HTTP/1.1\r\nHost: edock\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {len(data)}\r\n\r\n".encode() + data)
        out = b""
        while b"\r\n\r\n" not in out and (chunk := s.recv(65536)):
            out += chunk
        head, _, rest = out.partition(b"\r\n\r\n")
        status = int(head.split()[1])
        length = next((int(l.split(b":")[1]) for l in head.split(b"\r\n") if l.lower().startswith(b"content-length:")), None)
        while length is not None and len(rest) < length and (chunk := s.recv(65536)):
            rest += chunk
        s.close()
        return status, rest.decode(errors="replace")

    def log_path(self):
        return os.path.join(self.home, ".edock", "logs", "daemon.log")

    def errors(self):
        """ERROR lines in the engine log since this engine started (the log
        is shared with the engine before it)."""
        try:
            with open(self.log_path(), errors="replace") as f:
                f.seek(self.log_from)
                return [l for l in f if '"level":"error"' in l]
        except OSError:
            return []


COMPOSE = """services:
  web:
    image: nginx:alpine
    restart: unless-stopped
  db:
    image: redis:alpine
    restart: unless-stopped
    volumes: [data:/data]
volumes:
  data: {}
"""

ON_DEMAND = f"""services:
  web:
    image: nginx:alpine
    ports: ["{WEB_PORT + 1}:80"]
    labels:
      app.edock.on-demand: "1h"
"""


def make_state(old, version):
    """What the old engine leaves: returns what it has, for comparing."""
    out = old.sh(f"""
set -e
$D pull -q alpine:latest >/dev/null
$D pull -q nginx:alpine >/dev/null
$D pull -q redis:alpine >/dev/null
$D volume create up-vol >/dev/null
$D run --rm -v up-vol:/v alpine sh -c 'echo kept-{version} > /v/file; mkdir -p /v/dir; echo deep > /v/dir/deep'
$D network create up-net >/dev/null
$D run -d --name up-web --restart unless-stopped --network up-net -p 127.0.0.1:{WEB_PORT}:80 -v up-vol:/usr/share/nginx/html/data -l app.test=upgrade nginx:alpine >/dev/null
$D run --name up-exited --user 1234:5678 -e KEEP=me -w /tmp alpine sh -c 'exit 3' || true
$D run -d --name up-always --restart always alpine sleep 100000 >/dev/null
$D stop -t 1 up-always >/dev/null
$D run -d --name up-unless --restart unless-stopped alpine sleep 100000 >/dev/null
$D stop -t 1 up-unless >/dev/null
$D run --name up-commit alpine sh -c 'echo committed > /root/c'
$D commit up-commit up-img:1 >/dev/null
$D rm up-commit >/dev/null
""")
    if out:
        say("    (setup said:", out[:300], ")")
    proj = os.path.join(old.home, "proj")
    os.makedirs(proj, exist_ok=True)
    open(os.path.join(proj, "compose.yaml"), "w").write(COMPOSE)
    old.sh(f"cd {proj} && $D compose -p upproj up -d >/dev/null 2>&1")
    # Redis takes a moment to listen.
    old.sh(f"for i in $(seq 50); do $D exec upproj-db-1 redis-cli set k v{version} 2>/dev/null | grep -q OK && break; sleep 0.2; done; $D exec upproj-db-1 redis-cli save >/dev/null")
    has_on_demand = old.api("GET", "/edock/on-demand")[0] == 200
    if has_on_demand:
        od = os.path.join(old.home, "od")
        os.makedirs(od, exist_ok=True)
        open(os.path.join(od, "compose.yaml"), "w").write(ON_DEMAND)
        old.sh(f"cd {od} && $D compose -p upod up -d >/dev/null 2>&1")
        status, body = old.api("POST", "/edock/on-demand/park", {"name": "upod"})
        if status != 200:
            say("    (couldn't park upod:", body[:200], ")")
    # Running containers have stopped by the time the engine has, so what's
    # compared is names and the restart policies' running set.
    before = {
        "containers": sorted(old.sh("$D ps -a --format '{{.Names}}'").split()),
        "running": sorted(old.sh("$D ps --format '{{.Names}}'").split()),
        "volumes": sorted(v for v in old.sh("$D volume ls -q").split() if v.startswith(("up", "upproj"))),
        "networks": sorted(n for n in old.sh("$D network ls --format '{{.Name}}'").split() if n.startswith("up")),
        "images": sorted(old.sh("$D images --format '{{.Repository}}:{{.Tag}}'").split()),
        "on_demand": has_on_demand,
    }
    return before


def verify(new, before, version):
    after_names = sorted(new.sh("$D ps -a --format '{{.Names}}'").split())
    check("every container is there", after_names == before["containers"], f"{after_names} vs {before['containers']}")
    # Restart policies: back, unless stopped by hand.
    deadline = time.time() + 60
    running = []
    while time.time() < deadline:
        running = sorted(new.sh("$D ps --format '{{.Names}}'").split())
        if {"up-web", "up-always", "upproj-web-1", "upproj-db-1"} <= set(running):
            break
        time.sleep(1)
    check("restart policies brought up-web and the project back", {"up-web", "upproj-web-1", "upproj-db-1"} <= set(running), running)
    # As Docker: `always` comes back when the engine starts, even stopped by
    # hand; `unless-stopped` doesn't.
    check("up-always (stopped by hand) comes back with the engine, as on Docker", "up-always" in running, running)
    check("up-unless (unless-stopped, stopped by hand) stays stopped", "up-unless" not in running, running)
    # The web container serves its volume's file on its published port.
    page = ""
    for _ in range(50):
        try:
            page = urllib.request.urlopen(f"http://127.0.0.1:{WEB_PORT}/data/file", timeout=2).read().decode().strip()
            break
        except OSError:
            time.sleep(0.2)
    check("up-web serves the volume's file on its port", page == f"kept-{version}", page)
    check("the volume keeps its files", new.sh("$D run --rm -v up-vol:/v alpine cat /v/file /v/dir/deep") == f"kept-{version}\ndeep")
    check("up-web is still on up-net", "up-net" in new.sh("$D inspect -f '{{json .NetworkSettings.Networks}}' up-web"))
    check("up-web keeps its label", new.sh("$D inspect -f '{{index .Config.Labels \"app.test\"}}' up-web") == "upgrade")
    check("up-exited keeps its exit code, user, env and workdir", new.sh("$D inspect -f '{{.State.ExitCode}} {{.Config.User}} {{.Config.WorkingDir}}' up-exited") == "3 1234:5678 /tmp" and "KEEP=me" in new.sh("$D inspect -f '{{json .Config.Env}}' up-exited"))
    check("up-exited starts again, and exits as before", new.sh("$D start -a up-exited; echo exit=$?") .endswith("exit=3"))
    check("the volumes are there", sorted(v for v in new.sh("$D volume ls -q").split() if v.startswith(("up", "upproj"))) == before["volumes"])
    check("the network is there", sorted(n for n in new.sh("$D network ls --format '{{.Name}}'").split() if n.startswith("up")) == before["networks"])
    after_images = sorted(new.sh("$D images --format '{{.Repository}}:{{.Tag}}'").split())
    check("the images are there", set(before["images"]) <= set(after_images), f"{before['images']} vs {after_images}")
    check("a pulled image runs without pulling again", new.sh("$D run --rm --pull never alpine echo ran") == "ran")
    check("the committed image runs", new.sh("$D run --rm up-img:1 cat /root/c") == "committed")
    check("the project's redis kept its data", new.sh(f"$D exec upproj-db-1 redis-cli get k") == f"v{version}")
    check("compose sees the project", "upproj-web-1" in new.sh("$D compose -p upproj ps --format '{{.Name}}'"))
    if before["on_demand"]:
        state = new.sh("$D ps -a --filter name=upod-web-1 --format '{{.Status}}'")
        check("the parked project is still parked", state.endswith("(parked)"), state)
        page = ""
        try:
            page = urllib.request.urlopen(f"http://127.0.0.1:{WEB_PORT + 1}/", timeout=30).read().decode()
        except OSError as e:
            page = repr(e)
        check("the parked project wakes on a request", "Welcome to nginx" in page, page[:120])
    errors = new.errors()
    check("no ERROR in the new engine's log", not errors, "".join(errors[:3])[:400])


def damaged_records(binary):
    """containers.json with one record this engine can't read (one an eDock
    after it wrote): the others come back, and the file is kept as it was."""
    say("== a containers.json with a record that doesn't read")
    home = tempfile.mkdtemp(prefix="eudmg-", dir=TMP)
    eng = Engine(binary, home, "new")
    try:
        eng.start()
        eng.sh("$D pull -q alpine:latest >/dev/null; $D create --name keep-a alpine true >/dev/null; $D create --name odd-b alpine true >/dev/null; $D create --name keep-c alpine true >/dev/null")
        eng.stop()
        records = os.path.join(home, "Library", "Application Support", "eDock", "containers.json")
        saved = json.load(open(records))
        for r in saved:
            if r["name"] == "odd-b":
                r["status"] = "a-state-from-the-future"
        open(records, "w").write(json.dumps(saved))
        eng.start()
        names = sorted(eng.sh("$D ps -a --format '{{.Names}}'").split())
        check("the records that read come back", names == ["keep-a", "keep-c"], names)
        check("the file is kept as it was", os.path.exists(os.path.join(os.path.dirname(records), "containers.json.unreadable")))
        check("the engine says so", any("didn't read" in l for l in eng.errors()), eng.errors()[:1])
    finally:
        eng.stop()
        shutil.rmtree(home, ignore_errors=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("versions", nargs="*", default=DEFAULT)
    ap.add_argument("--new", default=os.path.join(ROOT, "target", "release", "edock"))
    ap.add_argument("--keep", action="store_true", help="leave each version's HOME for a look")
    args = ap.parse_args()
    for port in (WEB_PORT, WEB_PORT + 1, ROUTER_HTTP, ROUTER_HTTPS):
        with socket.socket() as s:
            if s.connect_ex(("127.0.0.1", port)) == 0:
                sys.exit(f"upgrade: port {port} is taken")
    for version in args.versions:
        say(f"== from {version}")
        app = release(version)
        home = tempfile.mkdtemp(prefix=f"eu{version.replace('.', '')}-", dir=TMP)
        old = Engine(os.path.join(app, "Contents", "MacOS", "edock"), home, f"old-{version}")
        new = Engine(args.new, home, "new")
        try:
            old.start()
            before = make_state(old, version)
            say(f"    {version} made {len(before['containers'])} containers, {len(before['volumes'])} volumes" + (", a parked project" if before["on_demand"] else ""))
            old.stop()
            new.start()
            verify(new, before, version)
        finally:
            old.stop()
            new.stop()
            if not args.keep:
                shutil.rmtree(home, ignore_errors=True)
    damaged_records(args.new)
    say(f"\n{len(args.versions)} versions: {'all came back' if not failures else f'{len(failures)} checks failed'}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
