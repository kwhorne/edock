# Releasing

A release is a signed, notarized `eDock-<version>-arm64.dmg` on GitHub. `scripts/bundle.sh` makes it.

## Once per Mac

1. **A Developer ID Application certificate** in your keychain. Check it with:

   ```sh
   security find-identity -v -p codesigning
   ```

2. **Notarization credentials** in the keychain, under a profile name. Use an app-specific password from appleid.apple.com. Type it at the prompt, never on the command line or in a chat:

   ```sh
   xcrun notarytool store-credentials edock-notary --apple-id <apple-id> --team-id <team-id>
   ```

3. **The engine's guest**, built once: `cargo xtask kernel && cargo xtask disk && cargo xtask guest` in `engine/`.

4. **The update key** in `~/.config/edock/update-key`. Installed copies of eDock accept an update only when it's signed with this key. Its public half is `UPDATE_KEY` in `desktop/src/main.rs`.
   - **The key already exists.** Copy it into a password manager or another safe place. If it's lost, installed copies can't take updates any more until people install a new version by hand.
   - **On a new Mac**, copy the file there with mode `600`, or point `EDOCK_UPDATE_KEY` at it.
   - **Never commit it.**
   - Only when starting over with a new key: `scripts/update-key/target/release/update-key keygen`. Then put the public key it prints into `UPDATE_KEY`. Copies already installed will only accept updates signed with the old key.

## Make a release

```sh
EDOCK_SIGN_ID="Developer ID Application: <name> (<team-id>)" \
EDOCK_NOTARY_PROFILE=edock-notary \
EDOCK_RELEASE_NOTES=notes.md \
  scripts/bundle.sh --version 0.3.0 --notarize
```

This takes about 15 minutes, most of it waiting for Apple. It:

1. **Sets the version** in `desktop/elyra.toml`, `desktop/Cargo.toml` and `engine/Cargo.toml`. One number then covers the app, its About box and the engine's `/version`. Without `--version`, the script refuses to build when the three differ.
2. **Builds the engine** (`cargo xtask host`, `cargo xtask bundle`).
3. **Builds the app** (`npm run build`, `rata bundle`).
4. **Puts the engine into the app:** `Contents/MacOS/edock`, plus `Contents/Resources/guest`, the kernel, initramfs and disk template. Kiri's licence goes in `Contents/Resources/engine`.
5. **Signs:**
   - the engine, with the `com.apple.security.virtualization` entitlement;
   - then the app around it.

   Both use the hardened runtime and a secure timestamp. `rata bundle` signs with `--deep`, which would drop the engine's entitlement, so the engine is signed again on its own. The script checks the entitlement is there.
6. **Notarizes the app** and staples its ticket, so the copy in Applications opens without a network.
7. **Makes the update:**
   - `dist/eDock-<version>-arm64.zip`, the stapled app, zipped with `--norsrc` so it stays sealed;
   - `dist/latest.json`, which names that zip with its SHA-256, the update key's signature and the notes from `EDOCK_RELEASE_NOTES`.

   Before any of this, the script checks that the update key matches the app's `UPDATE_KEY`, so a mismatch stops it at once rather than after 15 minutes.
8. **Makes the `.dmg`** (the app and a link to Applications), signs it, notarizes it, and staples it.
9. **Checks both with `spctl`**, and writes `dist/eDock-<version>-arm64.dmg.sha256`.

If Apple rejects either submission, the script prints Apple's log with the reason, and stops.

## Publish

```sh
git add -A && git commit -m "eDock 0.3.0"
git tag -a v0.3.0 -m "eDock 0.3.0"
git push origin main v0.3.0
gh release create v0.3.0 dist/eDock-0.3.0-arm64.dmg dist/eDock-0.3.0-arm64.dmg.sha256 \
  dist/eDock-0.3.0-arm64.zip dist/latest.json \
  --title "eDock 0.3.0" --notes-file notes.md --latest
```

The release must be marked **latest**, and must carry `latest.json` and the zip, for installed copies to find it. `latest.json` names the zip at `releases/download/v0.3.0/…`, so the tag and the version must agree.

Then check what people will download:

```sh
gh release download v0.3.0 -D /tmp/check && cd /tmp/check
shasum -a 256 -c eDock-0.3.0-arm64.dmg.sha256
xcrun stapler validate eDock-0.3.0-arm64.dmg
```

And what an installed eDock will get:

```sh
cd desktop && cargo test published_release -- --ignored
```

This test fetches the published `latest.json`, downloads the zip it names, and checks the zip's signature against the app's `UPDATE_KEY` and its SHA-256, just as an update does before installing.

## Local builds

Without `--notarize` the app is signed ad hoc. It runs on the Mac that built it, and Gatekeeper refuses it elsewhere.

```sh
scripts/bundle.sh                 # desktop/target/release/bundle/eDock.app
scripts/bundle.sh --dmg           # … and dist/eDock-<version>-arm64.dmg
scripts/bundle.sh --skip-engine   # reuse the engine already built
```

## In CI

An App Store Connect API key replaces the keychain profile:

```sh
EDOCK_NOTARY_KEY=/path/AuthKey_XXXX.p8 EDOCK_NOTARY_KEY_ID=XXXX EDOCK_NOTARY_ISSUER=<issuer-uuid> \
EDOCK_SIGN_ID="Developer ID Application: …" scripts/bundle.sh --version 0.3.0 --notarize
```

The CI machine's keychain still needs the Developer ID certificate.

## Versions

eDock uses semantic versions (`MAJOR.MINOR.PATCH`). An engine built from a checkout without a release version reports `0.0.0`, and `edock` skips its engine version check for that.
