# Updates

eDock checks for a new version **each time it starts**. Choose **Check for Updates…** in the menu bar item's menu to check at any other time. **Check for updates** on the Engine page and in the ⌘K palette do the same.

## Installing an update

When there's a newer version, a notice in the window offers it:

1. **↑ Update available: v0.3.0**, with **What's new** to read the release notes, **Install** and **Later**.
2. **Install** downloads and checks the update, showing its progress.
3. **v0.3.0 is installed. Restart to use it.** **Restart** opens the new version now. **Later** leaves it for the next time you open eDock. Nothing restarts until you say so.

eDock replaces itself only in a folder you can write to, normally Applications. See [Troubleshooting](../troubleshooting.md#updates) if installing refuses.

## Then restart the engine

The engine that was running keeps running the previous version, so your containers aren't interrupted by the update. The Engine page notices and offers **Restart engine**. Press it when it suits you. Running containers stop and start again, and an engine you'd stopped stays stopped.

## How an update is checked

An update is installed only if **all** of these hold:

- **Its signature.** The download is signed with eDock's update key, and eDock carries the public half of that key.
- **Its checksum.** The download matches the SHA-256 the release lists.
- **Its Developer ID.** The app inside is signed with eDock's Apple Developer ID for this app, and is unaltered since it was signed.
- **It's newer.** It's a newer version than the one running. An older release is never offered.

If any check fails, nothing is installed and the running eDock is left as it was.

The check asks GitHub for the newest release's `latest.json` over HTTPS, through the [proxy](../configuration.md#proxies) if one is set. Nothing about you or your containers is sent.
