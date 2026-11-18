import type { WritePlan } from "./data";

export type ItemKind = "skill" | "agent" | "command" | "hook" | "rule";
export type ItemStateName = "on" | "off" | "new" | "removed";

export interface SelectItem {
  id: string;
  kind: ItemKind;
  state: ItemStateName;
  hash: string;
  inSlice: boolean;
  command?: string;
  event?: string;
  matcher?: string | null;
  pending?: { hash: string; command: string };
}

export interface SelectOffer {
  kind: "switch-to-chosen-items";
  whole: string;
  slice: string;
  text: string;
}

export interface SelectPolicy {
  hooksOff: string | null;
  blocked: string | null;
}

export interface SelectLsData {
  source: string;
  tap: string;
  ref: string | null;
  slice: string;
  recorded: boolean;
  on: number;
  total: number;
  new: number;
  removed: number;
  items: SelectItem[];
  offer: SelectOffer | null;
  policy: SelectPolicy;
  warnings: string[];
}

export interface PlanRowData {
  kind: ItemKind;
  id: string;
  from?: string;
  to?: string;
  note?: string;
}

export interface PlanDiffs {
  added: PlanRowData[];
  dropped: PlanRowData[];
  changed: PlanRowData[];
  retick: PlanRowData[];
  fresh: PlanRowData[];
  gone: PlanRowData[];
  rules: PlanRowData[];
}

export interface LineCount {
  added: number;
  removed: number;
  files: number;
}

export interface HookText {
  event: string;
  matcher: string | null;
  command: string;
  was: string | null;
}

export interface SelectPlanData {
  source: string;
  slice: string;
  planHash: string;
  waiting: boolean;
  dryRun: boolean;
  ref: { pinned: string | null; latest: string };
  checked: boolean;
  fetchError: string | null;
  diffs: PlanDiffs;
  lines: Record<string, LineCount>;
  hooks: Record<string, HookText>;
  policy: SelectPolicy;
  offer: SelectOffer | null;
  plan: WritePlan;
  result: { applied: boolean; changed: string[]; undo?: string } | null;
}

export interface SelectSetData {
  source: string;
  dryRun: boolean;
  changed: string[];
  on: number;
  total: number;
  next: string;
}

export interface SelectUndoData {
  source: string;
  plan: WritePlan;
  result: { applied: boolean; changed: string[] } | null;
}

export interface SelectSources {
  sources: Array<{
    source: string;
    slice: string;
    ref: string | null;
    on: number;
    new: number;
    removed: number;
    total: number;
  }>;
  offers: Array<{ source: string; offer: SelectOffer }>;
}

export interface SetChanges {
  on: string[];
  off: string[];
  tick: string[];
}
