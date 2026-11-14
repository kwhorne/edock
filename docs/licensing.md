# Licensing

## The engine: a fork of Kiri

eDock's engine (`engine/`) is a fork of **[Kiri](https://github.com/libracoder/kiri)**, Copyright Canestack Labs and the Kiri contributors, licensed under the **Apache License 2.0**. It was forked from Kiri's commit `baa2d41` and modified by the eDock authors.

- `engine/LICENSE` is the Apache License 2.0, unchanged.
- `engine/NOTICE` is Kiri's notice file. Its first line is the attribution to Kiri, and the rest is Kiri's own, unchanged: the third-party notices for the Rust crates.
- `engine/UPSTREAM.md` lists what eDock changed. The main changes:
  - renamed Kiri to eDock;
  - removed Kiri's own window, its share service, its self-updater and its crash uploads, so eDock contacts none of Kiri's services;
  - dropped Windows support.

Apache-2.0 asks that redistributions keep the licence and the NOTICE, and say that files were changed. eDock ships both in the app, at `eDock.app/Contents/Resources/engine/`.

## The guest kernel: GPL-2.0

The Linux kernel that runs in eDock's VM is a separate work under the **GNU General Public License, version 2**. eDock builds it from the upstream kernel source with the configuration and patches in `engine/guest/kernel/`. `KERNEL_SOURCE_OFFER.md`, in the repository and in the app (`Contents/Resources/engine/`), says where to get the exact source.

## eDock's own code: MIT

eDock's own code is under the **MIT License** (`LICENSE` at the repository's root): the desktop app (`desktop/`), the scripts (`scripts/`) and this documentation (`docs/`). The app is built on the **[Elyra Framework](https://github.com/kwhorne/elyra-framework)**, which is MIT too.

It also uses:

| Component | Licence |
|---|---|
| [bollard](https://github.com/fussybeaver/bollard), the Docker API client | Apache-2.0 |
| [xterm.js](https://xtermjs.org), the terminal | MIT |
| [Svelte](https://svelte.dev) | MIT |

## What runs in containers

The images you run (nginx, k3s, …) carry their own licences. eDock doesn't ship them.
