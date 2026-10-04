import type { CommandRow } from "../bridge/data";
import type { Tier } from "../allcommands/model";
import type { ContextStatusData } from "../types/context";

export type Selection = "inherit" | "none" | string[];

export interface LayerRow {
  name: string;
  path: string;
  globs: string[];
  description: string;
}

export interface ProfileRow {
  name: string;
  shim: string;
  org: boolean;
  orgMode: string;
  rules: Selection;
  servers: Selection;
  generated: boolean;
  dir: string;
}

export interface ZshrcLine {
  line: number;
  file: string;
  text: string;
}

export interface DeadAlias {
  name: string;
  file: string;
  line: number;
  command: string;
}

export type CheckLevel = "ok" | "warn" | "fail";
export type Check = [CheckLevel, string];

export interface StatusView {
  layers: LayerRow[];
  profiles: ProfileRow[];
  legacyDupes: string[];
  shims: ContextStatusData["shims"];
  zshrc: {
    path: string;
    exists: boolean;
    legacyLines: ZshrcLine[];
    deadAliases: DeadAlias[];
  };
  config: ContextStatusData["config"];
}

export function statusView(data: ContextStatusData): StatusView {
  return {
    layers: data.layers as LayerRow[],
    profiles: data.profiles as ProfileRow[],
    legacyDupes: data.legacyDupes.map(String),
    shims: data.shims,
    zshrc: data.zshrc as StatusView["zshrc"],
    config: data.config,
  };
}

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
}

/** The tier and preview flag of a command come from the registry; a command it does not
 * list, or lists as planned, has no policy and the app does not run it. */
export function policyOf(rows: CommandRow[] | null, id: string): Policy | null {
  const row = rows?.find((c) => c.kind === "command" && c.id === id);
  if (!row || row.planned || !row.tier) return null;
  return {
    tier: row.tier,
    previewFlag: row.preview?.mode === "flag" ? row.preview.flag : null,
    terminal: row.surface === "terminal" || row.needs.includes("terminal-only"),
  };
}

export function selectionText(value: Selection, noun: string): string {
  if (value === "inherit") return `inherits ${noun}`;
  if (value === "none") return `no ${noun}`;
  return value.length === 0 ? `no ${noun}` : value.join(", ");
}

export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export function scopeText(layer: LayerRow): string {
  return layer.globs.length === 0 ? "always on" : layer.globs.join(", ");
}

export interface ShimFunction {
  name: string;
  what: string;
  /** False for a function that exists only in some setups. */
  always: boolean;
}

/** What the generated shims file defines. The default `claude` wrapper and `hrclaude` depend
 * on the config and on what the shell already has, the per-profile functions on the profiles. */
export function shimFunctions(profiles: ProfileRow[]): ShimFunction[] {
  return [
    {
      name: "mcpm_context_presync",
      what: "Before a launch: pulls the org tool when its interval has passed, then runs a context sync. Throttled, failures only warn.",
      always: false,
    },
    {
      name: "claude",
      what: "Runs the presync, then starts Claude with the default settings folder.",
      always: false,
    },
    {
      name: "hrclaude",
      what: "Only when your shell already has one: the same presync, then Claude under a compression policy.",
      always: false,
    },
    ...profiles.map((profile) => ({
      name: profile.shim,
      what: `Starts Claude in the launch profile ${profile.name} (${profile.org ? `org file by ${profile.orgMode}` : "no org file"}, ${selectionText(profile.servers, "servers")}).`,
      always: true,
    })),
  ];
}

const ORDER = /shims|sourced|source line|BEFORE/i;

/** The checks that are about the shell: is the file written, sourced, and sourced last. */
export function shellChecks(checks: Check[]): Check[] {
  return checks.filter(([, text]) => ORDER.test(text));
}

export function worst(checks: Check[]): CheckLevel {
  if (checks.some(([level]) => level === "fail")) return "fail";
  return checks.some(([level]) => level === "warn") ? "warn" : "ok";
}

export function folderLabel(root: string): string {
  const parts = root.split("/").filter(Boolean);
  return parts.slice(-2).join("/") || root;
}

export function formatTokens(n: number): string {
  return n.toLocaleString("en");
}
