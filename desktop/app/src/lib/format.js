export function bytes(n) {
  if (n == null || n < 0 || Number.isNaN(n)) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1000 && i < units.length - 1) {
    n /= 1000;
    i++;
  }
  return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

export function ago(seconds) {
  if (!seconds) return "—";
  const d = Date.now() / 1000 - seconds;
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  if (d < 86400 * 30) return `${Math.floor(d / 86400)} d ago`;
  if (d < 86400 * 365) return `${Math.floor(d / 86400 / 30)} mo ago`;
  return `${Math.floor(d / 86400 / 365)} y ago`;
}

export function shortId(id) {
  return (id ?? "").replace(/^sha256:/, "").slice(0, 12);
}

/** "running" → a badge variant. */
export function stateTone(state) {
  return (
    {
      running: "success",
      restarting: "warning",
      paused: "warning",
      created: "neutral",
      exited: "neutral",
      dead: "danger",
      removing: "warning",
    }[state] ?? "neutral"
  );
}

export function percent(n) {
  return `${(n ?? 0).toFixed(n >= 10 ? 0 : 1)}%`;
}
