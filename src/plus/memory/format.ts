export function sizeOf(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function dayOf(iso: string | null): string {
  return iso ? iso.slice(0, 10) : "never";
}

export const SETTER: Record<string, string> = {
  default: "Claude Code's default",
  global: "your global setting",
  repository: "this repository",
  project: "this repository",
  managed: "your organization",
  environment: "an environment variable",
};

export const setterOf = (setBy: string) => SETTER[setBy] ?? setBy;
