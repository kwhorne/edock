# Images, volumes and networks

The lists on these three pages work the same way:

- **Search** filters the rows.
- **Headers sort**; click again to reverse.
- **Column widths** can be dragged at a header's right edge. The app remembers them.
- **The lists update** when the engine reports a change. A container created or removed also updates which images, volumes and networks are in use.

## Images

One row per tag. An untagged image shows as `<none>`, under the repository it was pulled from.

| Column | |
|---|---|
| Repository, Tag | `nginx`, `alpine` |
| ID | The short image ID |
| Size | On disk |
| Created | When the image was built |
| In use | How many containers use it |

The header shows the number of images and the total size, counting an image tagged twice only once.

| Action | What it does |
|---|---|
| **Pull** | Asks for a reference (`nginx:latest`, `ghcr.io/org/app:1.2`) and pulls it, showing progress. A missing tag means `latest`. Credentials from `docker login` are used. |
| **Remove** (per row) | Removes that tag, or the image when it's untagged. An image a container uses can't be removed. |
| **Prune dangling** | Removes untagged images |
| **Prune unused** | Removes every image no container uses |

The prune actions say how much space they freed.

### An image

On eDock's engine, click an image to open it. The shield on a row opens its security scan straight away.

**Layers** shows each line of the Dockerfile that made the image:

- what it added, with a bar for its share of the image;
- a **base** badge on the lines that came with the base image;
- lines that changed no files (`ENV`, `CMD`…) in grey.

**Security** lists the image's known vulnerabilities, found by [Grype](https://github.com/anchore/grype).

- **Scan** reads the image from eDock's store on the Mac; the VM isn't woken.
- **The first scan** downloads Grype and its vulnerability database. That's about 200 MB to download, and it takes about 3 GB on disk in `~/Library/Application Support/eDock/scanner`. Later scans update the database first; offline, they use the one they have.
- **The results** show a count per severity, how many have a fix, and how many packages there are. Each vulnerability shows:
  - its severity;
  - its ID, which links to the advisory;
  - the package and version;
  - the versions that fix it;
  - the Dockerfile line that brought the package in, or *base image*.
- **Filter** by text, by severity, or to **Fixable only**.
- **Earlier scans** are kept (the last few), and a picker shows them.
- **SBOM** saves the scan's software bill of materials, as CycloneDX JSON, where you choose.

## Volumes

| Column | |
|---|---|
| Name | Hover to see where it is in the VM |
| Project | The Compose project that made it |
| Used by | The containers that mount it, or *unused* |
| Size | From `docker system df`. It can be blank on a busy engine, since the app won't wait more than 3 seconds for it. |
| Driver | Usually `local` |

| Action | What it does |
|---|---|
| **New volume** | Creates a named volume |
| **Import…** | Makes a new volume from a `.tar` (as **Export** writes them, or any tar of a folder's contents) |
| Folder | Opens the volume's page, with its files |
| Camera | Takes a [snapshot](snapshots.md) of its files |
| **Clone** | Copies it into a new volume, owners and all |
| **Export** | Saves its files to one `.tar`, owners kept: a seeded database to share, say |
| **Remove** | Removes a volume and its data, after asking. A volume in use can't be removed. |
| **Prune unused** | Removes every volume no container uses, named ones included, with their data |

### A volume

Click a volume for its page: what uses it, its project and driver, and **its files**, browsed as a container's are (see [Files](containers.md#files)): preview, save to the Mac, put files in, remove. No container of yours needs to run: a small `alpine` helper mounts the volume, with no network, and is removed when you leave the page (or after half an hour). It's one of eDock's own, so the list hides it.

Changes there are changes to the volume, seen at once by a container that uses it. For a database's files, stop the database first.

The page also has **Snapshot**, **Clone**, **Export** and **Remove**.

## Networks

| Column | |
|---|---|
| Name | Docker's own networks (`bridge`, `host`, `none`) are marked *built-in*; an internal network is marked *internal* |
| Driver | `bridge`, `host`, `null` |
| Subnet | The network's IPv4 subnet |
| Project | The Compose project that made it |
| Containers | The containers on it |

| Action | What it does |
|---|---|
| **New network** | Creates a bridge network |
| **Remove** | Removes a network (not a built-in one); one in use can't be removed |
| **Prune unused** | Removes every network no container uses |
