export interface SetupPart {
  id: string;
  label: string;
  count: number;
  default: boolean;
  sealed: boolean;
  error: string | null;
  notes: string[];
}

export interface PartsData {
  encryption: boolean;
  minPassphrase: number;
  lastExport: string | null;
  parts: SetupPart[];
}

export interface ExportData {
  dryRun: boolean;
  file: string;
  fileSize: number;
  name: string;
  parts: { id: string; label?: string; count?: number }[];
}

export interface RestoreRow {
  id: string;
  label: string;
  part: string;
  state: "add" | "here" | "conflict" | "needs" | "skip";
  note: string;
}

export interface RestoreData {
  applied: boolean;
  dryRun: boolean;
  plan: string;
  rows: RestoreRow[];
  summary: Record<RestoreRow["state"], number>;
  run?: string;
}

export interface CheckFinding {
  id: string;
  kind: string;
  repo: string;
  note: string;
  acked: boolean;
}

export interface CheckData {
  blockers: CheckFinding[];
  amber: CheckFinding[];
  carry: { id: string; label: string; ticked: boolean }[];
  ready: boolean;
  open: { blockers: number; carry: number };
  lastExport: string | null;
}

export const STATE_LABEL: Record<RestoreRow["state"], string> = {
  add: "Will be added",
  here: "Already here",
  conflict: "Differs, yours is kept",
  needs: "Needs you",
  skip: "Skipped",
};

/** The part whose values need `--secrets`; it comes only from the checklist. */
export const SECRETS_PART = "secrets";

/** `passphrase` says the passphrase goes on stdin; the caller sends it only when a sealed part is ticked. */
export function exportArgv(
  parts: string[],
  out: string,
  options: { passphrase: boolean; dryRun: boolean },
): string[] {
  const argv = ["setup", "export", "--parts", parts.join(","), "--out", out];
  if (parts.includes(SECRETS_PART)) argv.push("--secrets");
  if (options.passphrase) argv.push("--passphrase-stdin");
  if (options.dryRun) argv.push("--dry-run");
  return argv;
}

export function restoreArgv(
  file: string,
  options: { sealed: boolean; plan?: string; take?: string[] },
): string[] {
  const argv = ["setup", "restore", file];
  if (options.sealed) argv.push("--passphrase-stdin");
  for (const row of options.take ?? []) argv.push("--take", row);
  if (options.plan) argv.push("--apply", "--plan", options.plan);
  return argv;
}
