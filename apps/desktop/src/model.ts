/** `claude-opus-5-5` reads as "Claude Opus 5.5"; other IDs stay as they are. */
export const modelLabel = (raw: string) => {
  if (!raw.startsWith("claude-")) return raw;
  const parts = raw
    .slice(7)
    .split("-")
    .filter((p) => !/^\d{8}$/.test(p));
  const words = parts.filter((p) => !/^\d+$/.test(p)).map((p) => p[0].toUpperCase() + p.slice(1));
  const version = parts.filter((p) => /^\d+$/.test(p)).join(".");
  return ["Claude", ...words, version].filter(Boolean).join(" ");
};
