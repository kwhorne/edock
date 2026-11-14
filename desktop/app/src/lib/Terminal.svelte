<script>
  import { channel } from "@elyra/runtime";
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { api } from "../bindings";

  let { container } = $props();

  let el;
  let term;
  let fit;
  let sessionId = null;
  let unsub = null;
  let observer;
  let ended = $state(false);

  function css(name, fallback) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  async function open() {
    ended = false;
    term?.reset();
    fit.fit();
    const id = await api.exec_start(container, term.cols, term.rows, null);
    sessionId = id;
    unsub = channel(`exec:${id}`).subscribe((chunk) => {
      if (!chunk) return;
      if (chunk.data) term.write(chunk.data);
      if (chunk.ended) {
        ended = true;
        term.write("\r\n\x1b[2m[process exited — press Enter to reconnect]\x1b[0m\r\n");
      }
    });
    term.focus();
  }

  onMount(() => {
    term = new Terminal({
      fontFamily: css("--elyra-font-mono", "Menlo, monospace"),
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
      theme: {
        background: css("--elyra-inset", "#16161e"),
        foreground: css("--elyra-text", "#c0caf5"),
        cursor: css("--elyra-accent", "#7aa2f7"),
        selectionBackground: css("--elyra-accent-soft", "#283457"),
      },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    term.onData((data) => {
      if (ended) {
        if (data === "\r") {
          unsub?.();
          open();
        }
        return;
      }
      if (sessionId != null) api.exec_input(sessionId, data).catch(() => {});
    });
    term.onResize(({ cols, rows }) => {
      if (sessionId != null) api.exec_resize(sessionId, cols, rows).catch(() => {});
    });
    observer = new ResizeObserver(() => fit.fit());
    observer.observe(el);
    open().catch((e) => term.write(`\x1b[31m${e?.message ?? e}\x1b[0m\r\n`));
  });

  onDestroy(() => {
    observer?.disconnect();
    unsub?.();
    if (sessionId != null) api.stream_stop(sessionId);
    term?.dispose();
  });
</script>

<div class="terminal" bind:this={el}></div>
