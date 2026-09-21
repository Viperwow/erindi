const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["day", 86_400_000],
  ["hour", 3_600_000],
  ["minute", 60_000],
];

/** "5 minutes ago", "yesterday"; anything under a minute is "just now". */
export function ago(ms: number, now = Date.now()): string {
  const diff = now - ms;
  for (const [unit, size] of UNITS) {
    if (diff >= size) return rtf.format(-Math.round(diff / size), unit);
  }
  return "just now";
}
