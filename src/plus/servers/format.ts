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
