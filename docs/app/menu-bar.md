# The menu bar

eDock lives in the menu bar. Closing the main window hides it, and eDock keeps running there. It has no Dock icon while no window is open.

## The icon

- **The number beside it** is how many containers are running on the current engine. Nothing shows when none are.
- **Its tooltip** says the same, with the engine's name, for example *eDock — 3 running (edock)*. It also says when the engine isn't running.

## The popover

A **left click** opens a small window under the icon. It shows:

- the engine and how many containers run;
- **Start engine**, when the app is on eDock's engine and it isn't running or is stopped;
- **the running containers**, as `project/service` for Compose containers.
  - Up to two of each container's published ports, as links.
  - A **Stop** button.
- **the last five stopped containers**, each with a **Start** button.
- **Open eDock** and **Quit**.

Click a container's name to open it in the main window.

A click elsewhere, **Esc**, or another click on the icon closes the popover.

## The menu

A **right click** opens the menu:

| Item | What it does |
|---|---|
| Open eDock | Shows the main window |
| Check for Updates… | Asks whether a newer eDock is out (see [Updates](updates.md)) |
| Start eDock engine | Starts eDock's engine (as on the [Engine page](engine-and-contexts.md#edock-engine)) |
| Stop eDock engine | Stops it: containers stop and the VM powers off, until Start |
| Quit eDock | Quits the app |

Quitting eDock leaves the engine as it is, so `docker` keeps working while the app is closed. To turn the engine off, use **Stop eDock engine** before or after quitting: open eDock again and stop it, or run `edock` as in [Stopping the engine](../troubleshooting.md#stopping-the-engine).

## Opening eDock again

eDock runs once. Opening it again shows its main window instead of starting a second copy: from Finder, Spotlight or `open -a eDock`.
