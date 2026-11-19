#!/bin/bash
# The engine as it ships, checked before it goes out: e2e (engine/tests/e2e.sh)
# and Docker parity (engine/tests/parity.py, when OrbStack runs) against the
# engine binary and guest in DIR (an app's Contents, or engine/target/
# edock-engine), with nothing of the installed eDock touched.
#
#   scripts/verify.sh [DIR]          # default: engine/target/edock-engine
#
# The binary runs from a bundle of its own whose identifier makes it "eDock
# Gate" (see flavor.rs): its socket, settings, data, Finder folder, docker and
# kubectl contexts are all named for it, under a HOME of its own in $TMPDIR,
# and it leaves the routers' ports 80 and 443 alone. The same binary and the
# same guest files as DIR's: only where it keeps things differs.
#
# Exits non-zero when a check fails. e2e's soak isn't run (SOAK=0).

set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ENGINE="$ROOT/engine"
DIR=$(cd "${1:-$ENGINE/target/edock-engine}" && pwd)
[ -x "$DIR/MacOS/edock" ] && [ -d "$DIR/Resources/guest" ] || { echo "verify: no engine in $DIR (MacOS/edock and Resources/guest)" >&2; exit 2; }

REAL_HOME=$HOME
# Under /Users, as a Mac folder Linux machines and Finder see as it is (not
# $TMPDIR's /var/folders); short, for the socket's 104 bytes. Removed after.
H=$(mktemp -d "$REAL_HOME/.edock-verify.XXXXXX")
PLUGINS="$REAL_HOME/.docker/cli-plugins"
REF="${VERIFY_REFERENCE:-$REAL_HOME/.orbstack/run/docker.sock}"
LOGS="${VERIFY_LOGS:-$ENGINE/target/verify}"
rm -rf "$LOGS"
mkdir -p "$LOGS"
APP="$H/eDock Gate.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>app.edock.app.gate</string><key>CFBundleName</key><string>eDock Gate</string></dict></plist>
EOF
cp -p "$DIR/MacOS/edock" "$APP/Contents/MacOS/edock"
ln -s "$DIR/Resources/guest" "$APP/Contents/Resources/guest"
BIN="$APP/Contents/MacOS/edock"
SOCK="$H/.edock-gate/run/docker.sock"
export HOME="$H"

"$BIN" daemon > "$LOGS/engine.out" 2>&1 &
PID=$!
cleanup() {
  kill -TERM "$PID" 2>/dev/null
  for _ in $(seq 60); do kill -0 "$PID" 2>/dev/null || break; sleep 0.5; done
  kill -9 "$PID" 2>/dev/null
  # Its Finder folder, if it stayed mounted.
  for m in "$H/eDock Gate"; do mount | grep -qF " on $m " && umount "$m" 2>/dev/null; done
  cp "$H"/.edock-gate/logs/daemon.log "$LOGS/" 2>/dev/null
  rm -rf "$H" 2>/dev/null
}
trap cleanup EXIT
for _ in $(seq 100); do curl -sf --unix-socket "$SOCK" http://x/_ping >/dev/null 2>&1 && break; sleep 0.1; done
curl -sf --unix-socket "$SOCK" http://x/_ping >/dev/null || { echo "verify: the engine didn't start (see $LOGS/engine.out)" >&2; exit 1; }

# The docker CLI without a credential helper on PATH: e2e's logins stay in
# this run's config, never in your keychain (which this HOME hasn't).
mkdir -p "$H/bin"
ln -s "$(command -v docker)" "$H/bin/docker"
CLEAN=$(printf %s "$PATH" | tr ':' '\n' | while read -r d; do ls "$d"/docker-credential-* >/dev/null 2>&1 || printf '%s:' "$d"; done)
export PATH="$H/bin:${CLEAN%:}"

export DOCKER_CONFIG="$H/.docker"
mkdir -p "$DOCKER_CONFIG"
ln -s "$PLUGINS" "$DOCKER_CONFIG/cli-plugins"
unset DOCKER_HOST
docker context create edock-gate --docker "host=unix://$SOCK" >/dev/null

STATUS=0
echo "== e2e on $DIR"
(cd "$ENGINE" && DOCKER_CONTEXT=edock-gate EDOCK="$BIN" SOAK=0 bash tests/e2e.sh) > "$LOGS/e2e.log" 2>&1 || STATUS=1
grep -E "passed, .* failed" "$LOGS/e2e.log" || { echo "verify: e2e didn't finish (see $LOGS/e2e.log)"; STATUS=1; }
grep "  FAIL" "$LOGS/e2e.log" | sort -u

if [ -S "$REF" ]; then
  echo "== Docker parity against $REF"
  python3 "$ENGINE/tests/parity.py" --a "$SOCK" --b "$REF" > "$LOGS/parity.log" 2>&1 || STATUS=1
  tail -1 "$LOGS/parity.log"
  grep -A3 "^DIFF" "$LOGS/parity.log"
else
  echo "== Docker parity: skipped (no OrbStack; VERIFY_REFERENCE=SOCKET for another Moby)"
fi
[ "$STATUS" = 0 ] && echo "verify: all good" || echo "verify: FAILED (logs in $LOGS)"
exit "$STATUS"
