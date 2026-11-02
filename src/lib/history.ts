import type { Version } from "./types";

const startOfDay = (ts: number) => {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
};

export function dayLabel(ts: number, now: number) {
  const days = Math.round((startOfDay(now) - startOfDay(ts)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const d = new Date(ts);
  if (days < 7) return d.toLocaleDateString([], { weekday: "long" });
  if (d.getFullYear() === new Date(now).getFullYear()) return d.toLocaleDateString([], { month: "short", day: "numeric" });
  return d.toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
}

export interface TimelineNode {
  version: Version;
  /** Vertical space above this node (distance from the newer node). */
  space: number;
}

const sameDay = (a: number, b: number) => startOfDay(a) === startOfDay(b);

/**
 * Newest first. Spacing follows the log of the time between edits, with an extra
 * step when the day changes, so a day's edits cluster and days read as separate.
 */
export function timeline(versions: Version[]): TimelineNode[] {
  const desc = [...versions].sort((a, b) => b.createdAt - a.createdAt);
  return desc.map((v, i) => {
    const newer = desc[i - 1];
    if (!newer) return { version: v, space: 0 };
    const minutes = Math.max(1, (newer.createdAt - v.createdAt) / 60_000);
    const crossesDay = !sameDay(newer.createdAt, v.createdAt);
    const base = Math.min(30, 6 + Math.log10(minutes) * 7);
    return { version: v, space: Math.round(crossesDay ? base + 30 : base) };
  });
}
