<!-- A small line icon (Lucide-style paths), coloured by currentColor. -->
<script>
  let { name, size = 16 } = $props();

  const PATHS = {
    box: "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z M3.3 7 12 12l8.7-5 M12 22V12",
    layers: "m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z M2 12l8.58 3.91a2 2 0 0 0 1.66 0L21 12 M2 17l8.58 3.91a2 2 0 0 0 1.66 0L21 17",
    disk: "M22 12H2 M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z M6 16h.01 M10 16h.01",
    network: "M16 16h6v6h-6z M2 16h6v6H2z M9 2h6v6H9z M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3 M12 12V8",
    cpu: "M4 4h16v16H4z M9 9h6v6H9z M15 2v2 M15 20v2 M2 15h2 M2 9h2 M20 15h2 M20 9h2 M9 2v2 M9 20v2",
    play: "M6 3l14 9-14 9V3z",
    stop: "M6 6h12v12H6z",
    restart: "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8 M3 3v5h5",
    pause: "M6 4h4v16H6z M14 4h4v16h-4z",
    trash: "M3 6h18 M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6 M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2",
    terminal: "M4 17l6-6-6-6 M12 19h8",
    logs: "M8 6h13 M8 12h13 M8 18h13 M3 6h.01 M3 12h.01 M3 18h.01",
    chevron: "M9 18l6-6-6-6",
    external: "M15 3h6v6 M10 14 21 3 M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6",
    download: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3",
    plus: "M12 5v14 M5 12h14",
    broom: "M19.36 2.72 20.78 4.14 M15.07 7l-4.24 4.24 M8 14l-6 6 M14.5 8.5 9.33 13.67a3 3 0 0 0 0 4.24l.76.76a3 3 0 0 0 4.24 0L19.5 13.5",
    engine: "M12 2v4 M12 18v4 M4.93 4.93l2.83 2.83 M16.24 16.24l2.83 2.83 M2 12h4 M18 12h4 M4.93 19.07l2.83-2.83 M16.24 7.76l2.83-2.83",
    search: "M11 3a8 8 0 1 0 0 16 8 8 0 0 0 0-16z M21 21l-4.3-4.3",
    copy: "M8 8h12v12H8z M4 16V4h12",
    folder: "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z",
    back: "M15 18l-6-6 6-6",
    window: "M3 3h18v18H3z M3 9h18",
    hammer: "M15 12l-8.5 8.5a2.12 2.12 0 0 1-3-3L12 9 M17.64 15 22 10.64 M20.91 11.7l-1.25-1.25a2 2 0 0 1-.59-1.42V7.5l-2.84-2.83a4 4 0 0 0-5.65 0L9.5 5.75l3.09 3.08a2 2 0 0 1 .59 1.42v1.02l1.25 1.25",
    shield: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z M9 12l2 2 4-4",
    monitor: "M3 4h18v12H3z M8 20h8 M12 16v4 M7 8l3 2-3 2 M12 12h4",
    helm: "M12 2l8.5 4.5v11L12 22l-8.5-4.5v-11z M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z M12 2v6 M12 16v6 M3.5 6.5l5 3 M15.5 14.5l5 3 M20.5 6.5l-5 3 M8.5 14.5l-5 3",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={PATHS[name] ?? ""} />
</svg>
