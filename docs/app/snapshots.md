# Snapshots

A snapshot keeps a container's files, or a volume's, as they are now, so you can go back to them. Take one before an upgrade, a migration or anything risky.

Snapshots work on any engine the app shows: eDock, OrbStack, Docker Desktop and the rest.

## A container

**Snapshot** on a container's page takes one, with a note if you like. It's `docker commit`: an image named `edock-snapshots/<container>:<date-time>`, of the container's files, with its command and environment.

**Its volumes aren't in it.** A database's data usually lives in a volume, so snapshot the volume too.

**Restore** makes the container again from the snapshot, with the same name and settings (ports, mounts, networks, environment, restart policy), and starts it if it was running. The container it replaces is stopped and **kept**, renamed `<name>-before-<date-time>`. Remove it once you're happy. If the container is gone, Restore makes a new one from the snapshot.

## A volume

The camera on the **Volumes** page takes one. It's a `.tar.gz` of the volume's files, owners and permissions kept, in `~/.edock/snapshots/volumes/<volume>/` on your Mac. A short-lived `alpine` container writes it, with no network.

For a database, stop the containers that write to the volume first, as with any copy of its files.

**Restore** replaces what's in the volume with the snapshot. It's refused while a running container uses the volume, so stop that first. A volume that was removed is made again.

## The Snapshots page

**Snapshots** in the sidebar lists them all, newest first, with their age, size and note. Each has **Restore** and **Delete**.

A container's snapshot is an ordinary image, so it also shows on the **Images** page, and `docker run edock-snapshots/web:20261114-221503` works too.

## AI agents

Agents can list snapshots, and take and restore them after you say yes. See [AI agents](ai-agents.md).
