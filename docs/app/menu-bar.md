# The menu bar

eDock lives in the menu bar. Closing the main window hides it, and eDock keeps running there. It has no Dock icon while no window is open.

## The icon

- **The number beside it** is how many containers are running on the current engine. Nothing shows when none are.
- **A dot (•)** after it means a container ran out of memory or turned unhealthy, or containers kept waiting on memory, since you last opened the popover. Opening the popover takes the dot away.
- **Its tooltip** says the same, with the engine's name, for example *eDock — 3 running (edock)*. It also says when the engine isn't running, and when a container needs a look.

## The popover

A **left click** opens a small window under the icon. It shows:

- the engine and how many containers run;
- on eDock's engine, **the VM**: awake, with the memory its containers use and the most it may take, or asleep (it wakes on the next command). When containers wait on memory, it says so in amber (*memory is tight*) or red (*containers wait on memory*); click it for the Engine page;
- **Start engine**, when the app is on eDock's engine and it isn't running or is stopped;
- **Needs a look**: containers that ran out of memory or turned unhealthy in the last hour, and times the containers kept waiting on memory, newest first. Click one for its log (or the Engine page); **Clear** empties the list. They're listed whether or not [notifications](engine-and-contexts.md#notifications) are on.
- **Projects**: each Compose project as one row, with how many of its containers run (`2/3`), **Start all** and **Stop all**. A project that is [down](containers.md#projects-that-are-down) has **Up**, and one that is [parked](containers.md#on-demand) has **Wake**. The running ones come first.
- **Containers** outside Compose: the running ones, with up to two published ports and their address as links and a **Stop** button, then up to four stopped ones with **Start**. An unhealthy one has an amber dot.
- On eDock's engine, **Linux machines**: a running one has **Terminal**, a stopped one **Start**.
- **Open eDock**, **New service** (in the main window) and **Quit**.

Click a project, container or machine's name to open it in the main window.

A click elsewhere, **Esc**, or another click on the icon closes the popover.

## The menu

A **right click** opens the menu:

| Item | What it does |
|---|---|
| Open eDock | Shows the main window |
| Check for Updates… | Asks whether a newer eDock is out (see [Updates](updates.md)) |
| Start eDock engine | Starts eDock's engine (as on the [Engine page](engine-and-contexts.md#edock-engine)) |
| Stop eDock engine | Stops it: containers stop and the VM powers off, until Start (which starts again those with a restart policy) |
| Quit eDock | Quits the app |

Quitting eDock leaves the engine as it is, so `docker` keeps working while the app is closed. To turn the engine off, use **Stop eDock engine** before or after quitting: open eDock again and stop it, or run `edock` as in [Stopping the engine](../troubleshooting.md#stopping-the-engine).

## Opening eDock again

eDock runs once. Opening it again shows its main window instead of starting a second copy: from Finder, Spotlight or `open -a eDock`.
