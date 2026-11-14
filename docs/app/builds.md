# Builds

**Builds** lists each `docker build` on eDock, newest first, from BuildKit's own history. Builds made with eDock's builder show up after [Engine › Command line › Set up](engine-and-contexts.md#command-line), which creates the `edock-build` builder.

| Column | |
|---|---|
| Build | Its name: the tag, or the folder it was built from |
| Status | Completed, Running, Error or Canceled |
| Steps | How many, and how many came from the cache |
| Took | How long it ran |
| Started | When |

Click a build to see:

- **Its steps**, in the order they ran, each with its time and a *cached* badge when the cache had it. Click a step for its log.
- **Why it failed**, at the top: the error, the step, and the Dockerfile lines around it. The failed step's log is opened for you.
- **Warnings** from the Dockerfile checks, with the line and a link to more.

**Remove** takes a build off the history. Its image, if it made one, stays.

Reading the history wakes the builder (and the VM), so the app reads it only while this page is open. **Refresh** reads it again.

From a terminal:

```sh
edock builds                    # the history, as JSON
edock builds show BUILDER ID    # one build
edock builds rm BUILDER ID
```
