# Requests (HTTP inspector)

**Requests** shows each HTTP request to your containers through their domains, `http://<name>.edock.localhost` and `https://…`, with what the container answered. You don't need a proxy or any setup in the app you're testing.

## Record

Recording is off until you turn it on, with **Start recording** or the **Record** switch. It stays on, even after the engine restarts, until you turn it off. While it's off, the engine records nothing and the domain routers work as before.

The engine keeps the **last 300 requests** in memory, and the first **32 KB** of each body. Nothing is written to disk, and **Clear** forgets them all.

Only requests through container domains are recorded. Requests to a published port (`localhost:8080`) go straight to the container, so they aren't recorded.

## The list

Newest first, each with its time, method, path, container, status, content type, size and how long it took. A request still waiting shows **…**, and one that got no answer shows **failed**.

- **Filter** searches the method, host, path, container and status.
- The **status** picker shows 2xx, 3xx, 4xx or 5xx, or **Failed**: 4xx, 5xx and no answer.
- The **container** picker shows one container.

## A request

Click a request to open it beside the list:

- its status, the full URL, how long it waited for the response to start and how long it took in all, and the sizes;
- **Headers**: the request's and the response's;
- **Request** and **Response**: the bodies, as text. JSON is indented, and gzip and deflate are undone for a body within 32 KB (a longer compressed one shows as binary). A binary body shows its size.

| Button | |
|---|---|
| **Copy as curl** | The request as a `curl` command |
| **Replay** | Sends the request again, as recorded, and it shows as a new request. Not for a body longer than 32 KB, a binary or compressed body, or a WebSocket. |
| **Open** | Opens a GET in the browser |

A WebSocket shows as its `101 Switching Protocols`; the messages after it aren't recorded. HTTP/2 isn't recorded either: browsers use HTTP/1.1 with eDock's routers.

## AI agents

`http_requests` and `http_request` are among eDock's [MCP tools](ai-agents.md), so an agent can see which requests fail and what the container answered.
