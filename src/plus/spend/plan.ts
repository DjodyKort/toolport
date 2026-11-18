import type { PlanEntry, PlanProfile } from "./data";
import { boundedInt } from "./model";

export const CONTENT_FLAGS = [
  "OTEL_LOG_USER_PROMPTS",
  "OTEL_LOG_TOOL_DETAILS",
  "OTEL_LOG_TOOL_CONTENT",
];

const WRITES = new Set<PlanEntry["action"]>(["add", "update", "overwrite"]);

/** The profiles the person has not unticked, as the settings files to pass to `--profile`;
 * empty means every detected profile. */
export function chosenProfiles(profiles: PlanProfile[], off: string[]): string[] {
  return off.length === 0
    ? []
    : profiles.filter((p) => !off.includes(p.profile)).map((p) => p.profile);
}

export function selectedProfiles(profiles: PlanProfile[], off: string[]): PlanProfile[] {
  return profiles.filter((p) => !off.includes(p.profile));
}

/** Keys that already hold another value in a selected profile; each needs an explicit choice. */
export function conflictKeys(profiles: PlanProfile[]): string[] {
  const keys = profiles.flatMap((p) =>
    p.entries.filter((e) => e.action === "conflict").map((e) => e.key),
  );
  return [...new Set(keys)];
}

export function conflictValue(profiles: PlanProfile[], key: string): string {
  for (const profile of profiles) {
    const entry = profile.entries.find((e) => e.key === key && e.action === "conflict");
    if (entry?.current) return entry.current;
  }
  return "";
}

/** Settings written when the plan is applied: every add and update, and each conflict the
 * person chose to replace. */
export function writeCount(profiles: PlanProfile[], replace: string[]): number {
  return profiles.reduce(
    (sum, profile) =>
      sum +
      profile.entries.filter(
        (e) =>
          WRITES.has(e.action) || (e.action === "conflict" && replace.includes(e.key)),
      ).length,
    0,
  );
}

export interface DiffLine {
  sign: "+" | "-" | " ";
  text: string;
  note?: string;
}

export function diffLines(entry: PlanEntry, replaced: boolean): DiffLine[] {
  const wanted = `"${entry.key}": "${entry.wanted}"`;
  const current = `"${entry.key}": "${entry.current ?? ""}"`;
  if (entry.action === "add") return [{ sign: "+", text: wanted }];
  if (entry.action === "already set")
    return [{ sign: " ", text: wanted, note: "already set" }];
  if (entry.action === "conflict" && !replaced)
    return [
      { sign: " ", text: current, note: "yours, kept" },
      { sign: " ", text: `wanted ${entry.wanted}`, note: "not written" },
    ];
  return [
    { sign: "-", text: current },
    { sign: "+", text: wanted },
  ];
}

export const MIN_PORT = 1024;
export const MAX_PORT = 65535;

export function validPort(value: string): number | null {
  return boundedInt(value, MIN_PORT, MAX_PORT);
}
