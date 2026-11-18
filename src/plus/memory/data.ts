export type SwitchPosition = "default" | "on" | "off";

export interface MemoryFile {
  bytes: number;
  description: string | null;
  index: boolean;
  modified: string | null;
  name: string;
  path: string;
  type: string | null;
}

export interface Retention {
  days: number;
  default: number;
  file: string;
  min: number;
  perRepository: boolean;
  source: string;
}

export interface InstructionRowData {
  bytes: number;
  canExclude: boolean;
  kind: string;
  lazy: boolean;
  loaded: boolean;
  name: string;
  note: string | null;
  path: string | null;
  scope: string;
  source: string;
  state: string;
  tokens: number;
}

export interface MemoryState {
  enabled: boolean;
  environment: boolean;
  file: string | null;
  global: boolean | null;
  locked: boolean;
  managed: boolean;
  project: boolean | null;
  setBy: string;
  setByHand: boolean;
  switch: SwitchPosition;
  toolportOwns: boolean;
}

export interface SessionTotals {
  bytes: number;
  count: number;
  expiringThisWeek: number;
  oldest: string | null;
}

export interface TrashEntry {
  id: string;
  [key: string]: unknown;
}

export interface MemoryStatusData {
  cwd: string;
  directory: { custom: boolean; exists: boolean; path: string };
  instructions: {
    basis: string;
    excluded: number;
    imports: number;
    leftOutTokens: number;
    localFiles: number;
    memoryIndexTokens: number;
    rows: InstructionRowData[];
    totalTokens: number;
  };
  memory: {
    bytes: number;
    count: number;
    lastWritten: string | null;
    topicFiles: number;
  };
  notes: string[];
  repository: {
    git: boolean;
    key: string;
    linked: boolean;
    name: string;
    root: string;
    worktrees: Array<{ current: boolean; exists: boolean; path: string }>;
  };
  sessions: { retention: Retention; totals: SessionTotals };
  state: MemoryState;
  trash: TrashEntry[];
}

export interface MemoryLsData {
  cwd: string;
  directory: string;
  files: MemoryFile[];
  sharedBy: string[];
}

export interface PlanData {
  effects: Record<string, unknown>;
  steps: Array<{ detail: string; op: string; path: string }>;
  summary: string;
  undo: string | null;
  warnings: string[];
}

export interface MemorySetData {
  dryRun: boolean;
  name: string;
  plan: PlanData;
  position: SwitchPosition;
  previous: SwitchPosition;
  state: { enabled: boolean; setBy: string; switch: SwitchPosition };
}

export interface MemoryClearData {
  alsoOff: boolean;
  dryRun: boolean;
  files: MemoryFile[];
  forever: boolean;
  name: string;
  plan: PlanData;
  result: { applied: boolean; trashId?: string; undo?: string } | null;
  sharedBy: string[];
  totals: { bytes: number; files?: number };
}

export interface SessionRow {
  ageDays: number;
  bytes: number;
  daysLeft: number;
  expiring: boolean;
  folder: string;
  id: string;
  modified: string;
}

export interface SessionsLsData {
  archive: TrashEntry[];
  name: string;
  note: string | null;
  retention: Retention;
  sessions: SessionRow[];
  totals: SessionTotals;
  trash: TrashEntry[];
}

export interface SessionsClearData {
  dryRun: boolean;
  name: string;
  plan: PlanData;
  result: { applied: boolean; id?: string; undo?: string } | null;
  sessions: SessionRow[];
  to: "trash" | "archive";
  totalBytes: number;
}

export interface UndoData {
  dryRun: boolean;
  id?: string;
  name?: string;
  plan: PlanData;
  result: { applied: boolean } | null;
}

export interface RetentionData {
  note: string | null;
  retention: Retention;
}

export interface OverviewRow {
  key: string;
  lastActive: string | null;
  name: string;
  root: string;
  worktrees: number;
  instructions: {
    excluded: number;
    files: number;
    flags: string[];
    localFiles: number;
    tokens: number;
  };
  memory: {
    bytes: number;
    enabled: boolean;
    files: number;
    lastWritten: string | null;
    locked: boolean;
    setBy: string;
    switch: SwitchPosition;
  };
  sessions: SessionTotals;
}

export interface OverviewData {
  filter: string | null;
  filters: string[];
  global: { enabled: boolean | null; file: string };
  other: { folders: number; memoryBytes: number; sessionBytes: number; sessions: number };
  retention: Retention;
  rows: OverviewRow[];
  totals: {
    bytes: number;
    memoryBytes: number;
    memoryFiles: number;
    repositories: number;
    sessionBytes: number;
    sessions: number;
  };
}
