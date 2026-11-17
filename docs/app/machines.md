# Linux machines

A Linux machine is a long-lived Linux environment on eDock's engine. It's like a small server you keep:

- **Your Mac user is in it**, with the same name and uid, and passwordless `sudo`.
- **`/Users` is at the same path as on the Mac**, so a project in `~/Code/app` is at `~/Code/app` in the machine too.
- **It keeps its own files** (installed packages, `/etc`, databases) until you delete it.

Machines run on eDock's engine. While the app shows another engine, the page offers to switch.

## Make one

On **Machines**, under **New machine**:

1. Give it a **name**: letters, digits and dashes.
2. Pick a **distribution**:
   - Ubuntu 24.04 (the default)
   - Debian 13
   - Alpine 3.22
   - Fedora 42
   - Arch Linux
   - Rocky Linux 9
   - openSUSE Tumbleweed
3. Tick **systemd** if you want services managed as on a real server. systemd runs as the machine's init, which makes the machine privileged.
4. Leave **Mac's SSH agent** ticked for `git` and `ssh` in the machine to use your Mac's keys (GitHub, servers, private package registries). The keys stay on the Mac: the machine only asks the Mac's agent to sign, through `SSH_AUTH_SOCK` (`/run/host-services/ssh-auth.sock`).
5. Press **Create**.

The first machine of a distribution pulls its image; after that, a new one is ready in seconds. Then its page opens, with a shell.

## Use one

A machine's page has four tabs:

- **Terminal:** a login shell as your user, in your home: `bash` when the distribution has it, otherwise `sh`. It's the same terminal as a container's, with colours, cursor keys and full-screen programs.
- **Files:** its files, as a [container's Files tab](containers.md#files). `/Users` is your Mac's: the Files tab never removes it, or a folder a mount is under.
- **Ports:** what it listens on, and ports to publish on the Mac, as a [container's Ports tab](containers.md#ports).
- **Stats:** its CPU, memory, network and disk, live.

From a terminal, the same shell:

```sh
edock machine shell dev
edock machine shell dev -- uname -a     # one command
```

On the **Machines** list, each machine has **Shell**, **Start** or **Stop**, **Restart** and **Delete**. Machines start again with the engine, unless you stopped them.

A machine made before eDock 0.6.0, or without the SSH agent, doesn't get it later: make it again with it ticked.

## SSH and editors

`ssh dev.edock` reaches the machine `dev`, and so does any tool that works over SSH: VS Code and Cursor with **Remote - SSH**, Zed, `scp`, `rsync`, `ssh -L`. Nothing listens on a port, and there's no password: the way in is the engine.

Turn it on once, for every machine: **Set up SSH…** on a machine's page, or `edock machine ssh on`. It:

- makes an SSH key of eDock's own, in `~/.edock/ssh`;
- writes a `Host *.edock` block to `~/.edock/ssh/config`: your user, that key, and `edock machine stdio %n` as the ProxyCommand;
- puts one line first in `~/.ssh/config`: `Include ~/.edock/ssh/config`. Nothing else in the file changes.

The first `ssh` to a machine installs `openssh-server` in it (with its own package manager), which takes a few seconds. After that, connecting takes a fraction of a second. sshd doesn't run in the background: each connection starts its own `sshd -i` through the engine, and it ends with the connection. A stopped machine is started.

With SSH on, a machine's page shows `ssh dev.edock` (click to copy) and **Open in VS Code** or **Open in Cursor** when they're installed, which open your home in the machine over SSH (the **Remote - SSH** extension does it). In Zed: **Open Remote Project**, with `dev.edock`.

Host keys aren't checked for `*.edock`: the way in is the engine's socket, which only you can open, and a machine made again has new ones.

**Turn off** on the page, or `edock machine ssh off`, takes the line out of `~/.ssh/config` again.

## Delete one

**Delete** removes the machine and everything that's only in it: its packages, its `/etc` and its own files. Your files in `/Users` are the Mac's, and stay.

## Under the hood

A machine is a container named `machine-<name>`, labelled `app.edock.machine`. **Containers** shows it when you turn on **eDock's own**, and a prune never removes it, even stopped. See [`edock machine`](../cli.md#edock-machine) for the command line.
