# Kubernetes

eDock runs a **single-node Kubernetes cluster** ([k3s](https://k3s.io)) in one container on its engine, as OrbStack and Docker Desktop do. The **Kubernetes** page starts and stops it, and shows what runs in it.

The cluster runs on **eDock's engine**. While the app shows another engine (OrbStack, say), the page offers to switch to eDock.

## Start the cluster

Press **Start Kubernetes**. The first start pulls k3s and takes about a minute; later starts take seconds. The page shows the engine's progress while it works.

Before the first start, you can tick **Make `edock` kubectl's current context**. Either way, kubectl gets an `edock` context:

```sh
kubectl --context edock get pods -A
```

Once it runs, the page shows:

- **the API server**, `https://127.0.0.1:6443` (the next free port if 6443 is taken), and whether the node is **ready**;
- **a kubectl command** you can copy.

| Button | What it does |
|---|---|
| **Stop** | Stops the cluster. Its state (deployments, volumes, everything) is kept, and **Start** carries on where it was. |
| **Delete** (bin) | Deletes the cluster and all its state, after asking. The `edock` kubectl context goes too. |

You can do the same from a terminal with `edock k8s start | stop | delete`. See the [CLI](../cli.md#edock-k8s).

## Pods

The **Pods** tab lists pods across the namespaces. Use the namespace picker to see one namespace. With **All namespaces**, the system namespaces (`kube-system`, `kube-public` and `kube-node-lease`) are hidden until you turn on **System namespaces**.

| Column | |
|---|---|
| Name | With its owner under it, for example `Deployment/web`. A ReplicaSet's pod shows its Deployment. A pod that's stuck says why under that, in a line (see below). |
| Namespace | |
| Status | As `kubectl get pods` prints it: Running, Pending, ContainerCreating, CrashLoopBackOff, ImagePullBackOff, Error, Completed, Terminating… |
| Ready | Ready containers out of all, `1/1` |
| Restarts | Across its containers |
| Age | |

Each row has:

- **Load from eDock**, on a pod that can't pull an image eDock has (see [Images from eDock](#images-from-edock)).
- **Logs** and **Shell**, which open the pod on that tab. The shell needs the pod to be running.
- **Restart** (on a Deployment's pods), which runs `kubectl rollout restart`. The Deployment's pods are replaced one by one.
- **Delete**, which deletes the pod. A Deployment, ReplicaSet or StatefulSet then makes a new one.

Click a row to open the pod.

Pods change without the engine saying so, because they run in the node's own containerd. So the page asks for the list every 3 seconds while it's open and the window is visible.

## A pod

The pod's page shows its ready count, restarts, pod IP, age and owner.

A pod that's stuck says **why** at the top, in Kubernetes' own words:

- an image it can't pull (`ErrImagePull`, `ImagePullBackOff`), with what the registry said;
- a container that keeps exiting (`CrashLoopBackOff`), with how it ended last (`Error, exit code 1`): its log says why;
- a pod that can't be placed on the node (`0/1 nodes are available: 1 Insufficient memory`);
- any other reason a container waits.

A stuck pod opens on its **Events** tab. The tabs:

- **Logs:** the container's log, live, from its last 500 lines (`kubectl logs -f`). Filter, timestamps, wrap and copy work as for a [container's log](containers.md#logs). Kubernetes merges a pod's stdout and stderr, so lines aren't marked by stream.
- **Shell:** a terminal in the container (`kubectl exec -it`), with `bash` when the image has it, otherwise `sh`.
- **Containers:** each container's image, its state and reason with Kubernetes' message, its restarts, and how it last ended.
- **Events:** what Kubernetes said about the pod, oldest first, as `kubectl describe pod` lists them: scheduled, pulling, pulled, started, backing off… A warning is marked, and an event that repeated says how often. Kubernetes keeps events for an hour.

A pod with several containers has a **Container** picker, and the logs and shell follow it.

If the pod is deleted while you look at it, the page says so and keeps showing it as it was.

## Images from eDock

The cluster keeps its images in its own containerd, inside the node, not in eDock's image store. So an image you build or pull on eDock (`docker build -t myapp:dev .`) isn't in the cluster, and a pod that uses it can't pull it: `ErrImagePull`, since no registry has it.

**Load from eDock** puts it in the cluster, with no registry in between: on the pod's page (and its row) when eDock has the image, and as **Load into Kubernetes** on an [image's page](images-volumes-networks.md#an-image) while the cluster runs. From a terminal: `edock k8s load myapp:dev`.

Kubernetes tries the pull again by itself, waiting longer each time (up to five minutes). On a Deployment's pod, eDock offers to replace the pod right away, and the new one finds the image at once.

A pod uses a loaded image when its `imagePullPolicy` is `IfNotPresent` or `Never`. `IfNotPresent` is the default, except for an image tagged `:latest` (or with no tag), where it's `Always`: give your image another tag, such as `:dev`, or set `imagePullPolicy: IfNotPresent`.

Load it again after each build: the cluster keeps the copy it was given.

## Services

The **Services** tab lists services with their type, cluster IP and ports.

**LoadBalancer** services are published on the Mac's `localhost`, through the engine. Their ports are links, so click one to open it:

```sh
kubectl --context edock create deployment web --image=nginx:alpine
kubectl --context edock expose deployment web --port=8080 --target-port=80 --type=LoadBalancer
open http://localhost:8080
```

Other services show their ports as `port[:nodePort]/protocol`, with **Forward** next to each TCP port. Forward opens a port on the Mac, on `127.0.0.1`, that reaches the service inside the cluster, the way `kubectl port-forward` does, but without kubectl. The service then shows `localhost:<port>` as a link, and **■** stops the forward. A forward lives in the engine, so it stays when the app closes; it ends when Kubernetes or the engine stops.

## How the app reads the cluster

Everything the page reads or does runs `kubectl` **inside the node container** (`edock-k8s`), through the engine's exec API. Your Mac needs neither kubectl nor a kubeconfig for the app to work.

Pod and namespace names are checked before they reach kubectl, so `--all` and the like are refused.

## Settings

The cluster's settings are changed from the terminal. The node container is made again with them, and the cluster's state is kept.

```sh
edock k8s set --lan on            # the API server (and LoadBalancer services) on every interface, not only 127.0.0.1
edock k8s set --tls-san my-mac.local,192.168.1.20   # extra names in the API server's certificate
edock k8s set --port 6550         # another Mac port for the API server
```

Pods resolve `host.docker.internal` and `host.edock.internal` to the Mac.
