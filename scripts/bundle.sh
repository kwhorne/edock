#!/bin/sh
# Builds eDock.app: the Elyra desktop app (`rata bundle`) with the engine
# inside it, and for a release a signed, notarized and stapled .dmg.
#
#   eDock.app/Contents/MacOS/edock-desktop   the window and menu bar item
#   eDock.app/Contents/MacOS/edock           the engine (finds ../Resources/guest)
#   eDock.app/Contents/Resources/guest/      kernel, initramfs, data disk template
#
# The engine's guest kernel and disk must have been built once:
# `cargo xtask kernel && cargo xtask disk` in engine/. Its agent (the
# initramfs) is built again here every time, from the code being released.
#
# `rata bundle` signs the app with `codesign --deep`, which would strip the
# engine's virtualization entitlement, so the engine goes in after it and
# is signed on its own, and the app is sealed again around it.
#
# Usage:
#   scripts/bundle.sh                         eDock.app, ad-hoc signed (this Mac only)
#   scripts/bundle.sh --dmg                   … and dist/eDock-<version>-<arch>.dmg
#   scripts/bundle.sh --version 0.1.0 --notarize
#                                             a release: Developer ID, notarized, stapled
# Options:
#   --version X     set the version in desktop/elyra.toml, desktop/Cargo.toml and
#                   engine/Cargo.toml first (otherwise all three must already agree)
#   --skip-engine   reuse engine/target/edock-engine
#   --dmg           also make the .dmg
#   --notarize      sign with EDOCK_SIGN_ID, notarize and staple (implies --dmg),
#                   regenerate desktop/NOTICE, make the kernel source archive,
#                   and make the update: dist/eDock-<version>-<arch>.zip and
#                   dist/latest.json, signed with the update key
#
# Environment:
#   EDOCK_SIGN_ID        "Developer ID Application: Name (TEAMID)", or its SHA-1;
#                        "-" (the default) signs ad hoc
#   EDOCK_NOTARY_PROFILE a notarytool keychain profile, made once with
#                        `xcrun notarytool store-credentials <profile> --apple-id … --team-id …`
#   or, for CI, an App Store Connect API key:
#   EDOCK_NOTARY_KEY (path to the .p8), EDOCK_NOTARY_KEY_ID, EDOCK_NOTARY_ISSUER
#   EDOCK_UPDATE_KEY     the update signing key (default ~/.config/edock/update-key;
#                        see scripts/update-key)
#   EDOCK_RELEASE_NOTES  a Markdown file: the release's notes, shown in the update toast
#   EDOCK_RELEASE_REPO   where releases are published (default kwhorne/edock)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
RATA=${RATA:-rata}
command -v "$RATA" >/dev/null 2>&1 || RATA="$ROOT/../elyra-framework/target/release/rata"
SIGN_ID=${EDOCK_SIGN_ID:--}

ENGINE="$ROOT/engine"
UPDATE_KEY_TOOL="$ROOT/scripts/update-key/target/release/update-key"
REPO=${EDOCK_RELEASE_REPO:-kwhorne/edock}
DESKTOP="$ROOT/desktop"
APP="$DESKTOP/target/release/bundle/eDock.app"
DIST="$ROOT/dist"

SKIP_ENGINE=0 DMG=0 NOTARIZE=0 SET_VERSION=""
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-engine) SKIP_ENGINE=1 ;;
    --dmg) DMG=1 ;;
    --notarize) NOTARIZE=1 DMG=1 ;;
    --version) shift; SET_VERSION=${1:?--version needs a value} ;;
    --version=*) SET_VERSION=${1#--version=} ;;
    -h|--help) sed -n '2,36p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1 (see --help)"; exit 2 ;;
  esac
  shift
done

die() { echo "bundle: $*" >&2; exit 1; }

# ── Version: one number for the app, its About box and the engine ─────
version_of() { sed -n "s/^version = \"\(.*\)\"/\1/p" "$1" | head -1; }
if [ -n "$SET_VERSION" ]; then
  echo "$SET_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$' || die "--version $SET_VERSION isn't semver (1.2.3)"
  for f in "$DESKTOP/elyra.toml" "$DESKTOP/Cargo.toml" "$ENGINE/Cargo.toml"; do
    sed -i '' "s/^version = \".*\"/version = \"$SET_VERSION\"/" "$f"
  done
  # The lock files name the workspace's own versions too: bring them along
  # (only those: no dependency moves), so --locked builds and CI agree.
  (cd "$DESKTOP" && cargo update --workspace --offline -q)
  (cd "$ENGINE" && cargo update --workspace --offline -q)
fi
VERSION=$(version_of "$DESKTOP/elyra.toml")
for f in "$DESKTOP/Cargo.toml" "$ENGINE/Cargo.toml"; do
  v=$(version_of "$f")
  [ "$v" = "$VERSION" ] || die "$f says $v but desktop/elyra.toml says $VERSION: run with --version $VERSION"
done
ARCH=$(uname -m)
[ "$ARCH" = arm64 ] || die "eDock is built for Apple silicon only; this Mac is $ARCH"
echo "== eDock $VERSION ($ARCH)"

# ── Signing ────────────────────────────────────────────────────────────
if [ "$NOTARIZE" = 1 ]; then
  [ "$SIGN_ID" != "-" ] || die "--notarize needs EDOCK_SIGN_ID (a Developer ID Application identity)"
  if [ -n "${EDOCK_NOTARY_PROFILE:-}" ]; then
    NOTARY_MODE=profile
  elif [ -n "${EDOCK_NOTARY_KEY:-}" ] && [ -n "${EDOCK_NOTARY_KEY_ID:-}" ] && [ -n "${EDOCK_NOTARY_ISSUER:-}" ]; then
    NOTARY_MODE=key
  else
    die "--notarize needs EDOCK_NOTARY_PROFILE, or EDOCK_NOTARY_KEY + EDOCK_NOTARY_KEY_ID + EDOCK_NOTARY_ISSUER"
  fi
  # The update is signed with the key whose public half the app carries.
  (cd "$ROOT/scripts/update-key" && cargo build --release -q)
  "$UPDATE_KEY_TOOL" public >/dev/null || die "no update key: make one with $UPDATE_KEY_TOOL keygen (once), or set EDOCK_UPDATE_KEY"
  APP_KEY=$(sed -n 's/^const UPDATE_KEY: &str = "\(.*\)";/\1/p' "$DESKTOP/src/main.rs")
  [ "$("$UPDATE_KEY_TOOL" public)" = "$APP_KEY" ] || die "the update key isn't the one the app trusts (UPDATE_KEY in desktop/src/main.rs)"
  if [ -n "${EDOCK_RELEASE_NOTES:-}" ]; then [ -f "$EDOCK_RELEASE_NOTES" ] || die "EDOCK_RELEASE_NOTES: no file $EDOCK_RELEASE_NOTES"; fi
fi
if [ "$SIGN_ID" != "-" ]; then
  security find-identity -v -p codesigning | grep -qF "$SIGN_ID" || die "no signing identity \"$SIGN_ID\" in the keychain"
  # Notarization wants the hardened runtime and a secure timestamp.
  SIGN_OPTS="--options runtime --timestamp"
else
  SIGN_OPTS=""
fi
sign() { codesign --force --sign "$SIGN_ID" $SIGN_OPTS "$@"; }

# ── Build ──────────────────────────────────────────────────────────────
if [ "$SKIP_ENGINE" = 0 ]; then
  echo "== engine"
  for f in Image data.img; do
    [ -f "$ENGINE/guest/out/$f" ] || die "engine/guest/out/$f is missing: build the guest first (see engine/docs/development.md)"
  done
  # The guest agent too: an initramfs left from an older build would ship
  # an agent without the changes since.
  (cd "$ENGINE" && cargo xtask guest && cargo xtask host && cargo xtask bundle)
fi
[ -x "$ENGINE/target/edock-engine/MacOS/edock" ] || die "no engine bundle: run without --skip-engine"
ENGINE_VERSION=$(cd "$ENGINE" && cargo pkgid -p edock 2>/dev/null | sed 's/.*[#@]//')
[ "$ENGINE_VERSION" = "$VERSION" ] || [ "$SKIP_ENGINE" = 1 ] || die "the engine built as $ENGINE_VERSION, not $VERSION"

if [ "$NOTARIZE" = 1 ]; then
  # A release carries current notices: the app's from what it's built of.
  echo "== notices"
  "$ROOT/scripts/notices.sh"
fi

echo "== desktop"
(cd "$DESKTOP/app" && npm run build --silent)
(cd "$DESKTOP" && "$RATA" bundle)

echo "== engine into eDock.app"
cp "$ENGINE/target/edock-engine/MacOS/edock" "$APP/Contents/MacOS/edock"
rm -rf "$APP/Contents/Resources/guest"
cp -R "$ENGINE/target/edock-engine/Resources/guest" "$APP/Contents/Resources/guest"
mkdir -p "$APP/Contents/Resources/engine"
for f in LICENSE NOTICE KERNEL_SOURCE_OFFER.md; do
  cp "$ENGINE/target/edock-engine/Resources/$f" "$APP/Contents/Resources/engine/$f"
done
# The app's own licence, and its third-party notices.
cp "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE"
cp "$DESKTOP/NOTICE" "$APP/Contents/Resources/NOTICE"
xattr -cr "$APP"   # Finder info and the like break a signature

echo "== sign ($([ "$SIGN_ID" = "-" ] && echo "ad hoc" || echo "Developer ID"))"
sign --entitlements "$ENGINE/crates/edock/edock.entitlements" "$APP/Contents/MacOS/edock"
sign "$APP"
codesign --verify --deep --strict "$APP"
codesign -d --entitlements - "$APP/Contents/MacOS/edock" 2>/dev/null | grep -q com.apple.security.virtualization \
  || die "the engine lost its virtualization entitlement"
du -sh "$APP"

# notarytool with the credentials chosen above, each argument quoted.
notary() {
  sub=$1; shift
  if [ "$NOTARY_MODE" = profile ]; then
    xcrun notarytool "$sub" "$@" --keychain-profile "$EDOCK_NOTARY_PROFILE"
  else
    xcrun notarytool "$sub" "$@" --key "$EDOCK_NOTARY_KEY" --key-id "$EDOCK_NOTARY_KEY_ID" --issuer "$EDOCK_NOTARY_ISSUER"
  fi
}

notarize() {
  echo "== notarize $(basename "$1") (usually a few minutes)"
  out=$(notary submit "$1" --wait --output-format plist) || true
  status=$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || echo "")
  id=$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || echo "")
  if [ "$status" != "Accepted" ]; then
    echo "$out" >&2
    [ -n "$id" ] && notary log "$id" >&2 || true
    die "notarization of $(basename "$1") came back \"${status:-no answer}\""
  fi
  echo "accepted ($id)"
}

# ── Notarize the app, so the copy in /Applications carries its ticket ──
if [ "$NOTARIZE" = 1 ]; then
  ZIP="$DESKTOP/target/release/bundle/eDock-notarize.zip"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  rm -f "$ZIP"
  xcrun stapler staple "$APP"

  # The update: the stapled app as a zip (--norsrc: AppleDouble files would
  # break its seal), and latest.json naming it, with its SHA-256 and the
  # update key's signature. The app finds latest.json at
  # github.com/$REPO/releases/latest/download/latest.json.
  mkdir -p "$DIST"
  ZIP_NAME="eDock-$VERSION-$ARCH.zip"
  rm -f "$DIST/$ZIP_NAME"
  ditto -c -k --keepParent --norsrc "$APP" "$DIST/$ZIP_NAME"
  set -- "$VERSION" "https://github.com/$REPO/releases/download/v$VERSION/$ZIP_NAME" "$DIST/$ZIP_NAME"
  [ -n "${EDOCK_RELEASE_NOTES:-}" ] && set -- "$@" "$EDOCK_RELEASE_NOTES"
  "$UPDATE_KEY_TOOL" manifest "$@" > "$DIST/latest.json"
  echo "== update: $DIST/$ZIP_NAME, $DIST/latest.json"

  # GPL-2.0: the guest kernel's complete corresponding source, published
  # beside the binaries (guest/kernel/SOURCE_OFFER.md names this file).
  echo "== kernel source"
  (cd "$ENGINE" && scripts/ci/kernel-source.sh "$DIST/eDock-$VERSION-kernel-source.tar.xz")
fi

# ── The .dmg: eDock.app and a link to /Applications ────────────────────
if [ "$DMG" = 1 ]; then
  mkdir -p "$DIST"
  DMG_PATH="$DIST/eDock-$VERSION-$ARCH.dmg"
  STAGE=$(mktemp -d)
  trap 'rm -rf "$STAGE"' EXIT
  ditto "$APP" "$STAGE/eDock.app"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG_PATH"
  echo "== dmg"
  hdiutil create -quiet -volname "eDock $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG_PATH"
  [ "$SIGN_ID" = "-" ] || codesign --force --sign "$SIGN_ID" --timestamp "$DMG_PATH"
  if [ "$NOTARIZE" = 1 ]; then
    notarize "$DMG_PATH"
    xcrun stapler staple "$DMG_PATH"
    xcrun stapler validate "$DMG_PATH"
    spctl -a -t exec -vv "$APP"
    spctl -a -t open --context context:primary-signature -vv "$DMG_PATH"
  fi
  (cd "$DIST" && shasum -a 256 "$(basename "$DMG_PATH")" > "$(basename "$DMG_PATH").sha256")
  du -sh "$DMG_PATH"
  echo "== built $DMG_PATH"
else
  echo "== built $APP"
fi
