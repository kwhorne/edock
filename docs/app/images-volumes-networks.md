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
| **Remove** | Removes a volume and its data, after asking. A volume in use can't be removed. |
| **Prune unused** | Removes every volume no container uses, named ones included, with their data |

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
