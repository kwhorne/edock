<script>
  import { errorText } from "./state.svelte.js";
  import { channel, shell } from "@elyra/runtime";
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { SearchAddon } from "@xterm/addon-search";
  import { WebLinksAddon } from "@xterm/addon-web-links";
  import "@xterm/xterm/css/xterm.css";
  import { api } from "../bindings";
  import Icon from "./Icon.svelte";

  // A shell in a container, or what `start(cols, rows)` opens (a pod's).
  let { container = null, start = null } = $props();

  let el;
  let term;
  let fit;
  let search;
  // ⌘F: find in what the terminal shows and has scrolled by.
  let finding = $state(false);
  let query = $state("");
  let found = $state(null); // { index, count }
  let findInput = $state(null);
  // A link under the pointer: ⌘-click opens it.
  let hovered = $state(null);
  let sessionId = null;
  let unsub = null;
  let observer;
  let ended = $state(false);
  // Gone while a shell was being opened: close it as soon as it's there,
  // rather than leave it running in the container.
  let destroyed = false;

  function css(name, fallback) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
  }

  async function open() {
    ended = false;
    term?.reset();
    fit.fit();
    const id = start ? await start(term.cols, term.rows) : await api.exec_start(container, term.cols, term.rows, null);
    if (destroyed) {
      api.stream_stop(id);
      return;
    }
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

  function theme() {
    return {
      background: css("--elyra-inset", "#16161e"),
      foreground: css("--elyra-text", "#c0caf5"),
      cursor: css("--elyra-accent", "#7aa2f7"),
      selectionBackground: css("--elyra-accent-soft", "#283457"),
    };
  }

  onMount(() => {
    term = new Terminal({
      fontFamily: css("--elyra-font-mono", "Menlo, monospace"),
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 5000,
      theme: theme(),
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    // Links open with ⌘-click, so a plain click still selects text.
    term.loadAddon(
      new WebLinksAddon(
        (e, uri) => {
          if (e.metaKey) shell.openExternal(uri);
        },
        { hover: (_, uri) => (hovered = uri), leave: () => (hovered = null) },
      ),
    );
    search = new SearchAddon();
    term.loadAddon(search);
    search.onDidChangeResults((r) => (found = r ? { index: r.resultIndex, count: r.resultCount } : null));
    term.open(el);
    // ⌘F, ⌘G and ⇧⌘G are the window's, not the shell's.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown" || !e.metaKey) return true;
      const k = e.key.toLowerCase();
      if (k === "f") {
        openFind();
        return false;
      }
      if (k === "g") {
        e.shiftKey ? findPrev() : findNext();
        return false;
      }
      return true;
    });
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
    // Light and dark: the toolbar's switch sets data-theme, the system
    // setting the media query; the terminal's colours follow both.
    themeWatch = new MutationObserver(() => (term.options.theme = theme()));
    themeWatch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    dark.addEventListener("change", onScheme);
    open().catch((e) => term.write(`\x1b[31m${errorText(e)}\x1b[0m\r\n`));
  });

  // What a match looks like: the theme's own colours.
  function marks() {
    const accent = css("--elyra-accent", "#7aa2f7");
    return { matchBackground: css("--elyra-accent-soft", "#283457"), matchOverviewRuler: accent, activeMatchBackground: accent, activeMatchColorOverviewRuler: accent };
  }
  function openFind() {
    finding = true;
    const sel = term.getSelection();
    if (sel && !sel.includes("\n")) query = sel;
    queueMicrotask(() => findInput?.select());
  }
  const opts = () => ({ decorations: marks(), incremental: false });
  function findNext() {
    if (query) search.findNext(query, opts());
  }
  function findPrev() {
    if (query) search.findPrevious(query, opts());
  }
  function closeFind() {
    finding = false;
    found = null;
    search.clearDecorations();
    term.focus();
  }
  function findKey(e) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeFind();
    } else if (e.key === "Enter" || (e.metaKey && e.key.toLowerCase() === "g")) {
      e.preventDefault();
      e.shiftKey ? findPrev() : findNext();
    }
  }
  function typed() {
    if (query) search.findNext(query, { ...opts(), incremental: true });
    else {
      search.clearDecorations();
      found = null;
    }
  }

  let themeWatch;
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  const onScheme = () => term && (term.options.theme = theme());

  onDestroy(() => {
    destroyed = true;
    themeWatch?.disconnect();
    dark.removeEventListener("change", onScheme);
    observer?.disconnect();
    unsub?.();
    if (sessionId != null) api.stream_stop(sessionId);
    term?.dispose();
  });
</script>

<div class="terminal-wrap">
  <div class="terminal" bind:this={el}></div>
  {#if finding}
    <div class="term-find" role="search">
      <input bind:this={findInput} bind:value={query} oninput={typed} onkeydown={findKey} placeholder="Find" aria-label="Find in the terminal" spellcheck="false" />
      <span class="muted count">{query ? (found?.count ? `${found.index + 1} of ${found.count}` : "none") : ""}</span>
      <button class="icon-btn" title="Previous (⇧⌘G)" aria-label="Previous match" onclick={findPrev}><span class="rot up"><Icon name="chevron" size={13} /></span></button>
      <button class="icon-btn" title="Next (⌘G)" aria-label="Next match" onclick={findNext}><span class="rot down"><Icon name="chevron" size={13} /></span></button>
      <button class="icon-btn" title="Close (Esc)" aria-label="Close find" onclick={closeFind}><Icon name="close" size={13} /></button>
    </div>
  {/if}
  {#if hovered}<div class="term-link-hint">⌘-click to open {hovered}</div>{/if}
</div>
