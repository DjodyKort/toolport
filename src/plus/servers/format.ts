export function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Names of clients as the registry records them (`claude-code`, `cursor`), made readable. */
export function clientName(id: string): string {
  return id
    .replace(/@.*$/, "")
    .split(/[-_]/)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}

export const MAX_WAIT_SECONDS = 20;

const DAY_MS = 86_400_000;

/** `09:41` for today, `Thu 09:41` for an earlier day. */
export function since(ms: number, now = Date.now()): string {
  const sameDay = new Date(ms).toDateString() === new Date(now).toDateString();
  if (sameDay || now - ms < 0) return clock(ms);
  const day = new Date(ms).toLocaleDateString([], { weekday: "short" });
  return now - ms < 6 * DAY_MS
    ? `${day} ${clock(ms)}`
    : new Date(ms).toLocaleDateString();
}

/** `3 h`, `2 days`; whole units, rounded down, never shorter than a minute. */
export function span(seconds: number): string {
  const minutes = Math.max(1, Math.floor(seconds / 60));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} h`;
  return `${Math.floor(hours / 24)} days`;
}

/** An image digest as shown to a person: `sha256:` and the first twelve digits. */
export function shortVersion(version: string): string {
  const hex = version.startsWith("sha256:") ? version.slice(7) : null;
  return hex && hex.length > 12 ? `sha256:${hex.slice(0, 12)}...` : version;
}
