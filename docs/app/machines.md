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
4. Press **Create**.

The first machine of a distribution pulls its image; after that, a new one is ready in seconds. Then its page opens, with a shell.

## Use one

A machine's page has a **login shell** as your user, in your home: `bash` when the distribution has it, otherwise `sh`. It's the same terminal as a container's, with colours, cursor keys and full-screen programs.

From a terminal, the same shell:

```sh
edock machine shell dev
edock machine shell dev -- uname -a     # one command
```

On the **Machines** list, each machine has **Shell**, **Start** or **Stop**, **Restart** and **Delete**. Machines start again with the engine, unless you stopped them.

## Delete one

**Delete** removes the machine and everything that's only in it: its packages, its `/etc` and its own files. Your files in `/Users` are the Mac's, and stay.

## Under the hood

A machine is a container named `machine-<name>`, labelled `app.edock.machine`. **Containers** shows it when you turn on **eDock's own**, and a prune never removes it, even stopped. See [`edock machine`](../cli.md#edock-machine) for the command line.
