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
| Name | With its owner under it, for example `Deployment/web`. A ReplicaSet's pod shows its Deployment. |
| Namespace | |
| Status | As `kubectl get pods` prints it: Running, Pending, ContainerCreating, CrashLoopBackOff, ImagePullBackOff, Error, Completed, Terminating… |
| Ready | Ready containers out of all, `1/1` |
| Restarts | Across its containers |
| Age | |

Each row has:

- **Logs** and **Shell**, which open the pod on that tab. The shell needs the pod to be running.
- **Restart** (on a Deployment's pods), which runs `kubectl rollout restart`. The Deployment's pods are replaced one by one.
- **Delete**, which deletes the pod. A Deployment, ReplicaSet or StatefulSet then makes a new one.

Click a row to open the pod.

Pods change without the engine saying so, because they run in the node's own containerd. So the page asks for the list every 3 seconds while it's open and the window is visible.

## A pod

The pod's page shows its ready count, restarts, pod IP, age and owner, and has three tabs:

- **Logs:** the container's log, live, from its last 500 lines (`kubectl logs -f`). Filter, timestamps, wrap and copy work as for a [container's log](containers.md#logs). Kubernetes merges a pod's stdout and stderr, so lines aren't marked by stream.
- **Shell:** a terminal in the container (`kubectl exec -it`), with `bash` when the image has it, otherwise `sh`.
- **Containers:** each container's image, its state and reason, and its restarts.

A pod with several containers has a **Container** picker, and the logs and shell follow it.

If the pod is deleted while you look at it, the page says so and keeps showing it as it was.

## Services

The **Services** tab lists services with their type, cluster IP and ports.

**LoadBalancer** services are published on the Mac's `localhost`, through the engine. Their ports are links, so click one to open it:

```sh
kubectl --context edock create deployment web --image=nginx:alpine
kubectl --context edock expose deployment web --port=8080 --target-port=80 --type=LoadBalancer
open http://localhost:8080
```

Other services show their ports as `port[:nodePort]/protocol`, with **Forward** next to each TCP port. Forward opens a port on the Mac, on `127.0.0.1`, that reaches the service inside the cluster, the way `kubectl port-forward` does, but without kubectl. The service then shows `localhost:<port>` as a link, and **■** stops the forward. Forwards end when Kubernetes or the engine stops.

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
