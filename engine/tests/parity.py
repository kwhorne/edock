#!/usr/bin/env python3
"""Docker parity: the same docker commands against eDock and against another
engine that runs Moby (Docker's own: OrbStack, Docker Desktop), with what
always differs (ids, times, addresses) made equal, and the answers compared.

    tests/parity.py                         # eDock against OrbStack
    tests/parity.py --a SOCKET --b SOCKET   # any two engines
    tests/parity.py -k cp                   # only cases whose name has "cp"
    tests/parity.py -v                      # every case's answers, alike or not

Each case is a shell command; `$D` is the docker CLI pointed at the engine
under test. Only what it makes is touched: names start with `parity-`, and
they're removed before and after. Exits 1 when an answer differs, unless
the case is in KNOWN with the reason it does.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile

HOME = os.path.expanduser("~")
P = "parity"

# (name, shell command). Setup and teardown are commands too; `$D` is docker.
SETUP = f"""
$D network create {P}-net >/dev/null
$D volume create {P}-vol >/dev/null
$D run -d --name {P}-web --network {P}-net --network-alias web-alias nginx:alpine >/dev/null
$D run -d --name {P}-box alpine sleep 600 >/dev/null
$D run -d --name {P}-usr --user 1234:5678 alpine sleep 600 >/dev/null
$D create --name {P}-made alpine sh -c 'echo made; exit 4' >/dev/null
"""
TEARDOWN = f"""
$D rm -f $($D ps -aq --filter name={P}-) >/dev/null 2>&1
$D network rm {P}-net >/dev/null 2>&1
$D volume rm $($D volume ls -q --filter name={P}-) >/dev/null 2>&1
"""

CASES = [
    # ── users and groups ──
    ("user-default", "$D run --rm alpine id"),
    ("user-root", "$D run --rm --user root alpine id"),
    ("user-uid", "$D run --rm --user 1234 alpine id"),
    ("user-uid-gid", "$D run --rm --user 1234:5678 alpine id"),
    ("user-name", "$D run --rm --user nobody alpine id"),
    ("user-name-group", "$D run --rm --user nobody:nogroup alpine id"),
    ("user-unknown-name", "$D run --rm --user nosuchuser alpine id >/dev/null 2>&1; echo exit=$?"),
    ("user-unknown-group", "$D run --rm --user nobody:nosuchgroup alpine id >/dev/null 2>&1; echo exit=$?"),
    ("group-add-id", "$D run --rm --group-add 10 alpine id"),
    ("group-add-name", "$D run --rm --user nobody --group-add wheel alpine id"),
    ("user-home", "$D run --rm --user nobody alpine sh -c 'echo $HOME'"),
    # ── working directory, environment, hostname ──
    ("workdir-default", "$D run --rm alpine pwd"),
    ("workdir-set", "$D run --rm -w /tmp alpine pwd"),
    ("workdir-made", "$D run --rm -w /new/dir alpine sh -c 'pwd; ls -ld /new/dir | cut -c1-10'"),
    ("workdir-image", "$D run --rm nginx:alpine pwd"),
    ("env-default", "$D run --rm alpine sh -c 'env | grep -v ^HOSTNAME= | sort'"),
    ("env-set", "$D run --rm -e A=1 -e B='two words' -e EMPTY= alpine sh -c 'echo \"$A|$B|$EMPTY\"'"),
    ("env-image-keys", "$D run --rm nginx:alpine sh -c 'env | cut -d= -f1 | grep -v ^HOSTNAME | sort'"),
    ("env-file", "f=$(mktemp); printf 'X=1\\n# no\\nY=a b\\n' > $f; $D run --rm --env-file $f alpine sh -c 'echo \"$X|$Y\"'; rm -f $f"),
    ("hostname-set", "$D run --rm --hostname hal alpine hostname"),
    ("hostname-default-is-id", "$D run --rm alpine hostname | grep -cE '^[0-9a-f]{12}$'"),
    ("hosts-own-name", "$D run --rm --hostname hal alpine sh -c 'grep -c hal /etc/hosts'"),
    # ── commands, entrypoints and exit codes ──
    ("exit-code", "$D run --rm alpine sh -c 'exit 3'; echo exit=$?"),
    ("exit-false", "$D run --rm alpine false; echo exit=$?"),
    ("exit-not-found", "$D run --rm alpine nosuchcommand >/dev/null 2>&1; echo exit=$?"),
    ("exit-not-executable", "$D run --rm alpine /etc/passwd >/dev/null 2>&1; echo exit=$?"),
    ("entrypoint", "$D run --rm --entrypoint echo alpine hello world"),
    ("cmd-args", "$D run --rm alpine sh -c 'printf \"%s|\" \"$@\"' x 'a b' '' c"),
    ("stdin", "echo piped | $D run --rm -i alpine cat"),
    ("stderr-apart", "$D run --rm alpine sh -c 'echo out; echo err >&2' 2>/dev/null"),
    ("start-attach-exit", f"$D start -a {P}-made; echo exit=$?"),
    ("wait-exit", f"$D wait {P}-made"),
    ("stop-sigterm-sh", "c=$($D run -d alpine sleep 600); $D stop -t 1 $c >/dev/null; $D inspect -f '{{.State.ExitCode}}' $c; $D rm $c >/dev/null"),
    ("stop-trap", "c=$($D run -d alpine sh -c 'trap \"exit 7\" TERM; sleep 600 & wait'); sleep 0.5; $D stop $c >/dev/null; $D inspect -f '{{.State.ExitCode}}' $c; $D rm $c >/dev/null"),
    ("init-pid1", "$D run --rm --init alpine sh -c 'cat /proc/1/comm'"),
    ("no-init-pid1", "$D run --rm alpine sh -c 'cat /proc/1/comm'"),
    # ── exec ──
    ("exec-user", f"$D exec -u nobody {P}-box id"),
    ("exec-container-user", f"$D exec {P}-usr id"),
    ("exec-workdir", f"$D exec -w /etc {P}-box pwd"),
    ("exec-env", f"$D exec -e Z=9 {P}-box sh -c 'echo $Z'"),
    ("exec-exit", f"$D exec {P}-box sh -c 'exit 5'; echo exit=$?"),
    ("exec-stopped", f"$D exec {P}-made true >/dev/null 2>&1; echo exit=$?"),
    ("exec-stdin", f"echo in | $D exec -i {P}-box cat"),
    # ── docker cp ──
    ("cp-in-owner", f"d=$(mktemp -d); echo a > $d/a; $D cp $d/a {P}-box:/tmp/; $D exec {P}-box stat -c '%u:%g %a' /tmp/a; rm -rf $d"),
    ("cp-in-archive", f"d=$(mktemp -d); echo a > $d/a; $D cp -a $d/a {P}-usr:/tmp/; $D exec {P}-usr stat -c '%u:%g' /tmp/a; rm -rf $d"),
    ("cp-in-dir-contents", f"d=$(mktemp -d); mkdir -p $d/x/y; echo b > $d/x/y/b; $D exec {P}-box mkdir -p /tmp/into; $D cp $d/x/. {P}-box:/tmp/into; $D exec {P}-box find /tmp/into | sort; rm -rf $d"),
    ("cp-out-file", f"d=$(mktemp -d); $D cp {P}-box:/etc/alpine-release $d/r && ls $d; rm -rf $d"),
    ("cp-out-dir-contents", f"d=$(mktemp -d); $D exec {P}-box sh -c 'mkdir -p /tmp/o/s && echo z > /tmp/o/s/z'; $D cp {P}-box:/tmp/o/. $d/; find $d -type f | sed \"s#$d##\"; rm -rf $d"),
    ("cp-out-missing", f"$D cp {P}-box:/nope /tmp/parity-nope >/dev/null 2>&1; echo exit=$?"),
    ("cp-in-missing-dir", f"d=$(mktemp -d); echo a > $d/a; $D cp $d/a {P}-box:/nodir/ >/dev/null 2>&1; echo exit=$?; rm -rf $d"),
    ("cp-symlink-out", f"d=$(mktemp -d); $D exec {P}-box ln -sf /etc/hostname /tmp/link; $D cp {P}-box:/tmp/link $d/l; test -L $d/l && echo link || echo file; rm -rf $d"),
    ("cp-created", f"d=$(mktemp -d); echo a > $d/a; $D cp $d/a {P}-made:/tmp/ && echo ok; rm -rf $d"),
    # ── inspect ──
    ("inspect-state", f"$D inspect -f '{{{{.State.Status}}}} {{{{.State.Running}}}} {{{{.State.ExitCode}}}}' {P}-box {P}-made"),
    ("inspect-config", f"$D inspect -f '{{{{.Config.User}}}}|{{{{.Config.WorkingDir}}}}|{{{{json .Config.Cmd}}}}|{{{{json .Config.Entrypoint}}}}' {P}-usr {P}-web"),
    ("inspect-restart", "c=$($D create --restart unless-stopped alpine true); $D inspect -f '{{.HostConfig.RestartPolicy.Name}} {{.HostConfig.RestartPolicy.MaximumRetryCount}}' $c; $D rm $c >/dev/null"),
    ("inspect-ports", "c=$($D create -p 127.0.0.1:18765:80 -p 18766:81/udp alpine true); $D inspect -f '{{json .HostConfig.PortBindings}}' $c; $D rm $c >/dev/null"),
    ("inspect-mounts", f"c=$($D create -v {P}-vol:/data -v /tmp:/host:ro --tmpfs /run alpine true); $D inspect -f '{{{{range .Mounts}}}}{{{{.Type}}}} {{{{.Destination}}}} {{{{.RW}}}};{{{{end}}}}' $c | tr ';' '\\n' | sort; $D rm $c >/dev/null"),
    ("inspect-labels", "c=$($D create -l a=1 -l b alpine true); $D inspect -f '{{json .Config.Labels}}' $c; $D rm $c >/dev/null"),
    ("ps-format", f"$D ps -a --filter name={P}-made --format '{{{{.Names}}}}|{{{{.Image}}}}|{{{{.Command}}}}|{{{{.State}}}}'"),
    ("ps-status-exited", f"$D ps -a --filter name={P}-made --format '{{{{.Status}}}}'"),
    # ── networks, DNS, volumes ──
    ("dns-by-name", f"$D run --rm --network {P}-net alpine sh -c 'getent hosts {P}-web | wc -l'"),
    ("dns-by-alias", f"$D run --rm --network {P}-net alpine sh -c 'getent hosts web-alias | wc -l'"),
    ("dns-default-bridge", f"$D run --rm alpine sh -c 'getent hosts {P}-web | wc -l'"),
    ("http-by-name", f"$D run --rm --network {P}-net alpine wget -qO- http://{P}-web/ | grep -c 'Welcome to nginx'"),
    ("host-docker-internal", "$D run --rm alpine sh -c 'getent hosts host.docker.internal | wc -l'"),
    ("network-none", "$D run --rm --network none alpine sh -c 'ip -o link | wc -l'"),
    ("volume-persists", f"$D run --rm -v {P}-vol:/v alpine sh -c 'echo kept > /v/f'; $D run --rm -v {P}-vol:/v alpine cat /v/f"),
    ("volume-ro", f"$D run --rm -v {P}-vol:/v:ro alpine sh -c 'touch /v/x 2>/dev/null; echo exit=$?'"),
    ("volume-image-content", f"$D volume create {P}-vol2 >/dev/null; $D run --rm -v {P}-vol2:/etc/nginx nginx:alpine ls /etc/nginx | sort | head -3"),
    ("bind-mac-folder", "d=$(mktemp -d); echo mac > $d/f; $D run --rm -v $d:/m alpine sh -c 'cat /m/f; stat -c %u /m/f'; rm -rf $d"),
    ("tmpfs", "$D run --rm --tmpfs /scratch alpine sh -c 'mount | grep -c \" /scratch \"'"),
    ("read-only", "$D run --rm --read-only alpine sh -c 'touch /x 2>/dev/null; echo exit=$?'"),
    # ── limits and capabilities ──
    ("ulimit-nofile", "$D run --rm alpine sh -c 'ulimit -n'"),
    ("ulimit-set", "$D run --rm --ulimit nofile=512:1024 alpine sh -c 'ulimit -n; ulimit -Hn'"),
    ("cap-default-chown", "$D run --rm alpine sh -c 'touch /f && chown 1:1 /f; echo exit=$?'"),
    ("cap-drop", "$D run --rm --cap-drop CHOWN alpine sh -c 'touch /f && chown 1:1 /f 2>/dev/null; echo exit=$?'"),
    ("memory-limit", "$D run --rm -m 64m alpine sh -c 'cat /sys/fs/cgroup/memory.max'"),
    ("pids-limit", "$D run --rm --pids-limit 50 alpine sh -c 'cat /sys/fs/cgroup/pids.max'"),
    ("shm-size", "$D run --rm --shm-size 32m alpine sh -c \"df -k /dev/shm | awk 'NR==2{print \\$2}'\""),
    # ── capabilities, security ──
    ("cap-eff-default", "$D run --rm alpine sh -c 'grep CapEff /proc/self/status'"),
    ("cap-eff-privileged", "$D run --rm --privileged alpine sh -c 'grep CapEff /proc/self/status'"),
    ("cap-add-net-admin", "$D run --rm --cap-add NET_ADMIN alpine sh -c 'ip link add d0 type dummy && echo ok'"),
    ("no-net-admin", "$D run --rm alpine sh -c 'ip link add d0 type dummy 2>/dev/null; echo exit=$?'"),
    ("no-new-privs", "$D run --rm --security-opt no-new-privileges alpine sh -c 'grep NoNewPrivs /proc/self/status'"),
    ("seccomp-mode", "$D run --rm alpine sh -c 'grep Seccomp: /proc/self/status'"),
    ("seccomp-unconfined", "$D run --rm --security-opt seccomp=unconfined alpine sh -c 'grep Seccomp: /proc/self/status'"),
    # ── DNS and hosts ──
    ("resolv-user-network", f"$D run --rm --network {P}-net alpine sh -c 'grep nameserver /etc/resolv.conf'"),
    ("resolv-dns-flag", "$D run --rm --dns 9.9.9.9 --dns-search corp.example alpine sh -c 'grep -v ^# /etc/resolv.conf'"),
    ("add-host", "$D run --rm --add-host foo:10.1.2.3 alpine getent hosts foo"),
    ("add-host-gateway", "$D run --rm --add-host foo:host-gateway alpine sh -c 'getent hosts foo | wc -l'"),
    # Names only; eDock's host.docker.internal lines are its own (Docker
    # Desktop and OrbStack answer that name by DNS).
    ("hosts-lines", "$D run --rm alpine sh -c 'grep -v ^# /etc/hosts' | grep -v docker.internal | awk '{$1=\"\"; print}' | sort"),
    ("network-container", f"$D run --rm --network container:{P}-box alpine hostname | grep -cE '^[0-9a-f]{{12}}$'"),
    # ── ports from the Mac ──
    ("port-published", "c=$($D run -d -p 127.0.0.1:18781:80 nginx:alpine); for i in $(seq 50); do curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18781/ 2>/dev/null | grep -q 200 && break; sleep 0.1; done; curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:18781/; echo; $D port $c; $D rm -f $c >/dev/null"),
    ("port-random", "c=$($D run -d -P nginx:alpine); $D port $c 80 | sed 's/:[0-9]*$/:<port>/' | sort; $D rm -f $c >/dev/null"),
    # ── more of the CLI ──
    ("name-taken", f"$D run --name {P}-box alpine true >/dev/null 2>&1; echo exit=$?"),
    ("rm-running", f"$D rm {P}-box >/dev/null 2>&1; echo exit=$?"),
    ("image-missing", "$D run --rm parity-no/such-image:x true >/dev/null 2>&1; echo exit=$?"),
    ("rename", "c=$($D create --name parity-old alpine true); $D rename parity-old parity-new; $D inspect -f '{{.Name}}' $c; $D rm $c >/dev/null"),
    ("update-restart", "c=$($D create alpine true); $D update --restart always $c >/dev/null; $D inspect -f '{{.HostConfig.RestartPolicy.Name}}' $c; $D rm $c >/dev/null"),
    ("diff", f"$D exec {P}-box sh -c 'echo x > /etc/parity-new; rm -f /etc/motd'; $D diff {P}-box | grep -E 'parity-new|motd|^C /etc$' | sort"),
    ("export-has-files", f"$D export {P}-box | tar -t 2>/dev/null | grep -cE '^etc/alpine-release$'"),
    ("commit-run", f"$D exec {P}-box sh -c 'echo committed > /root/c'; $D commit {P}-box parity-img:c >/dev/null; $D run --rm parity-img:c cat /root/c; $D rmi parity-img:c >/dev/null"),
    ("commit-change", f"$D commit -c 'ENV X=7' -c 'CMD [\"sh\",\"-c\",\"echo $X\"]' {P}-box parity-img:d >/dev/null; $D run --rm parity-img:d; $D rmi parity-img:d >/dev/null"),
    ("top-header", f"$D top {P}-box | head -1 | tr -s ' '"),
    ("healthcheck-inspect", "c=$($D create --health-cmd 'true' --health-interval 5s --health-retries 2 alpine true); $D inspect -f '{{json .Config.Healthcheck}}' $c; $D rm $c >/dev/null"),
    ("health-status", "c=$($D run -d --health-cmd 'true' --health-interval 1s --health-start-interval 200ms --health-start-period 5s alpine sleep 60); for i in $(seq 40); do [ \"$($D inspect -f '{{.State.Health.Status}}' $c)\" = healthy ] && break; sleep 0.25; done; $D inspect -f '{{.State.Health.Status}} {{.State.Health.FailingStreak}}' $c; $D rm -f $c >/dev/null"),
    ("image-config", "$D image inspect -f '{{json .Config.Cmd}} {{json .Config.Entrypoint}} {{.Config.WorkingDir}}' nginx:alpine"),
    ("volume-inspect", f"$D volume inspect -f '{{{{.Driver}}}} {{{{.Scope}}}} {{{{.Name}}}}' {P}-vol"),
    ("network-inspect", f"$D network inspect -f '{{{{.Driver}}}} {{{{.Scope}}}} {{{{.Internal}}}} {{{{.Attachable}}}}' {P}-net"),
    ("stop-signal", "c=$($D create --stop-signal SIGINT --stop-timeout 3 alpine true); $D inspect -f '{{.Config.StopSignal}} {{.Config.StopTimeout}}' $c; $D rm $c >/dev/null"),
    ("tty", "$D run --rm -t alpine sh -c 'test -t 1 && echo tty || echo no-tty' | tr -d '\\r'"),
    ("entrypoint-cleared", "$D run --rm --entrypoint '' nginx:alpine echo cleared"),
    ("env-passthrough", "PARITY_X=from-mac $D run --rm -e PARITY_X alpine sh -c 'echo $PARITY_X'"),
    # ── logs ──
    ("logs-streams", "c=$($D run -d alpine sh -c 'echo out; echo err >&2'); $D wait $c >/dev/null; echo \"[$($D logs $c 2>/dev/null)] [$($D logs $c 2>&1 >/dev/null)]\"; $D rm $c >/dev/null"),
    ("logs-tail", "c=$($D run -d alpine sh -c 'seq 5'); $D wait $c >/dev/null; $D logs --tail 2 $c; $D rm $c >/dev/null"),
]

# Differences with a reason: they don't fail the run. Keep each one short and
# true; remove it when the difference goes.
KNOWN = {
    "ulimit-nofile": "the default comes from the engine's own settings, not from Moby: Docker Desktop and Docker Engine on Linux give 1048576, as eDock does; OrbStack gives 20480",
    "seccomp-unconfined": "eDock keeps a filter that refuses vsock sockets even unconfined or privileged, so no container can reach the Mac's side of the VM (engine/docs/features.md)",
    "top-header": "docker top runs ps on the engine's host: OrbStack's VM has busybox ps; Docker Engine's (procps) gives eDock's columns",
}

NORMALIZE = [
    (re.compile(r"\b[0-9a-f]{64}\b"), "<id>"),
    (re.compile(r"\b[0-9a-f]{12}\b"), "<shortid>"),
    (re.compile(r"\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})\b"), "<time>"),
    (re.compile(r"\b(?:\d{1,3}\.){3}\d{1,3}\b"), "<ip>"),
    (re.compile(r"(Less than a second|About a minute|About an hour|\d+ (seconds?|minutes?|hours?)) ago"), "<ago>"),
    (re.compile(r"/var/folders/\S+|/tmp/tmp\.\S+"), "<tmp>"),
]


def normal(text):
    for rx, to in NORMALIZE:
        text = rx.sub(to, text)
    return text.strip()


def run(sock, script, timeout=120):
    env = dict(os.environ, D=f"docker --host unix://{sock}", DOCKER_HOST=f"unix://{sock}")
    env.pop("DOCKER_CONTEXT", None)
    try:
        r = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True, timeout=timeout)
        return r.stdout + (("\n[stderr] " + r.stderr.strip()) if r.stderr.strip() else "")
    except subprocess.TimeoutExpired:
        return "[timed out]"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--a", default=f"{HOME}/.edock/run/docker.sock", help="the engine under test (eDock)")
    ap.add_argument("--b", default=f"{HOME}/.orbstack/run/docker.sock", help="the reference engine (Moby)")
    ap.add_argument("-k", default="", help="only cases whose name contains this")
    ap.add_argument("-v", action="store_true", help="show every case's answers")
    args = ap.parse_args()
    cases = [c for c in CASES if args.k in c[0]]
    for sock in (args.a, args.b):
        if not os.path.exists(sock):
            sys.exit(f"parity: no engine at {sock}")
        for image in ("alpine:latest", "nginx:alpine"):
            run(sock, f"$D image inspect {image} >/dev/null 2>&1 || $D pull -q {image} >/dev/null")
        run(sock, TEARDOWN)
        out = run(sock, SETUP)
        if out.strip():
            sys.exit(f"parity: setup failed on {sock}:\n{out}")
    differ, known = [], []
    try:
        for name, cmd in cases:
            a, b = normal(run(args.a, cmd)), normal(run(args.b, cmd))
            same = a == b
            if not same:
                (known if name in KNOWN else differ).append((name, a, b))
            if args.v or not same:
                mark = "same" if same else ("KNOWN" if name in KNOWN else "DIFF")
                print(f"{mark:5} {name}")
                if not same or args.v:
                    print("      eDock:", a.replace("\n", "\n             "))
                    if not same:
                        print("      Moby: ", b.replace("\n", "\n             "))
                    if name in KNOWN:
                        print("      why:  ", KNOWN[name])
    finally:
        for sock in (args.a, args.b):
            run(sock, TEARDOWN)
    print(f"\n{len(cases)} cases: {len(cases) - len(differ) - len(known)} the same, {len(known)} known differences, {len(differ)} new differences")
    sys.exit(1 if differ else 0)


if __name__ == "__main__":
    main()
