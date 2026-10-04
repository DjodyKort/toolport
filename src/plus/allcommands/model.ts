import type { CommandFlag, CommandRow, CommandsData } from "../bridge/data";

export type Tier = "read" | "write" | "destructive";
export type ToolRow = CommandsData["tools"][number];

export interface FormValues {
  operands: Record<string, string>;
  /** A bool flag is `true` when set; every other kind holds its text. */
  flags: Record<string, string | boolean>;
  /** The stdin field holds something (never what). */
  secretFilled: boolean;
}

export const emptyValues = (): FormValues => ({
  operands: {},
  flags: {},
  secretFilled: false,
});

const PASSPHRASE_STDIN = "--passphrase-stdin";
const MAX_PHRASE = 40;

export function commandRows(data: CommandsData): CommandRow[] {
  return data.commands.filter((row) => row.kind === "command");
}

export function groupCounts(rows: CommandRow[]): Array<{ group: string; count: number }> {
  const counts = new Map<string, number>();
  for (const row of rows) counts.set(row.group, (counts.get(row.group) ?? 0) + 1);
  return [...counts].map(([group, count]) => ({ group, count })).sort(byGroup);
}

const byGroup = (a: { group: string }, b: { group: string }) =>
  a.group.localeCompare(b.group);

export function matchesQuery(row: CommandRow, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const haystack = [row.id, row.summary, ...row.flags.map((flag) => flag.name)]
    .join(" ")
    .toLowerCase();
  return words.every((word) => haystack.includes(word));
}

export function isTerminal(row: CommandRow): boolean {
  return row.surface === "terminal" || row.needs.includes("terminal-only");
}

/** The flag that turns a run into a preview (or, for `unless-applied`, into the apply). The
 * page sets it itself, so the form does not offer it. */
export function managedFlag(row: CommandRow): string | null {
  return row.preview?.flag ?? null;
}

/** What the form offers: not the hidden flags (refused or test-only), not a flag that takes a
 * secret (the stdin field replaces it) and not the preview flag. */
export function formFlags(row: CommandRow): CommandFlag[] {
  const managed = managedFlag(row);
  return row.flags.filter(
    (flag) => !flag.hidden && !flag.sensitive && flag.name !== managed,
  );
}

/** The flags the app keeps out of the form because they would put a secret on a command
 * line (the stdin field stands in for `--passphrase-stdin`). */
export function withheldFlags(row: CommandRow): string[] {
  return row.flags
    .filter((flag) => flag.sensitive && flag.name !== PASSPHRASE_STDIN)
    .map((flag) => flag.name);
}

export type StdinNeed = "secret" | "payload";

/** A command that reads stdin: a secret when it also has a flag for one, else a payload. */
export function stdinNeed(row: CommandRow): StdinNeed | null {
  if (!row.needs.includes("stdin") || isTerminal(row)) return null;
  return row.flags.some((flag) => flag.sensitive) ? "secret" : "payload";
}

/** Splits on blanks; a quoted part keeps its blanks. */
export function splitWords(text: string): string[] {
  const words: string[] = [];
  for (const match of text.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)) {
    words.push(match[1] ?? match[2] ?? match[3]);
  }
  return words;
}

function textOf(value: string | boolean | undefined): string {
  return typeof value === "string" ? value.trim() : "";
}

function flagIsSet(flag: CommandFlag, values: FormValues): boolean {
  const value = values.flags[flag.name];
  if (flag.valueType === "bool") return value === true;
  return textOf(value) !== "";
}

function operandTokens(row: CommandRow, values: FormValues): string[] {
  return row.operands.flatMap((operand) => {
    const text = textOf(values.operands[operand.name]);
    if (text === "") return [];
    return operand.variadic ? splitWords(text) : [text];
  });
}

function flagTokens(row: CommandRow, values: FormValues): string[] {
  const tokens: string[] = [];
  for (const flag of formFlags(row)) {
    const value = values.flags[flag.name];
    if (flag.valueType === "bool") {
      if (value === true) tokens.push(flag.name);
    } else if (flag.repeatable) {
      for (const line of textOf(value).split("\n")) {
        if (line.trim() !== "") tokens.push(`${flag.name}=${line.trim()}`);
      }
    } else if (textOf(value) !== "") {
      tokens.push(`${flag.name}=${textOf(value)}`);
    }
  }
  if (stdinNeed(row) === "secret" && values.secretFilled) {
    if (row.flags.some((flag) => flag.name === PASSPHRASE_STDIN)) {
      tokens.push(PASSPHRASE_STDIN);
    }
  }
  return tokens;
}

/** The argv of the command as the form describes it, without the preview flag. */
export function baseArgv(row: CommandRow, values: FormValues): string[] {
  return [...row.path, ...operandTokens(row, values), ...flagTokens(row, values)];
}

/** Everything the form still lacks before the command can run, in plain words. */
export function problems(row: CommandRow, values: FormValues): string[] {
  const found: string[] = [];
  let gap: string | null = null;
  for (const operand of row.operands) {
    const text = textOf(values.operands[operand.name]);
    if (text === "") {
      if (operand.required) found.push(`${operand.name} is required`);
      gap ??= operand.name;
      continue;
    }
    if (gap && !operand.required) found.push(`Fill ${gap} before ${operand.name}`);
    const words = operand.variadic ? splitWords(text) : [text];
    if (words.some((word) => word.startsWith("-")))
      found.push(`${operand.name} cannot start with a dash`);
  }
  for (const flag of formFlags(row)) {
    const set = flagIsSet(flag, values);
    if (flag.required && !set) found.push(`${flag.name} is required`);
    if (
      set &&
      flag.valueType === "integer" &&
      !/^-?\d+$/.test(textOf(values.flags[flag.name]))
    )
      found.push(`${flag.name} must be a whole number`);
  }
  const need = stdinNeed(row);
  if (need === "secret" && !values.secretFilled)
    found.push("The value on stdin is required");
  for (const group of row.oneOf) {
    const satisfied = group.some((name) => {
      if (name === PASSPHRASE_STDIN) return values.secretFilled;
      const flag = row.flags.find((candidate) => candidate.name === name);
      return !!flag && !flag.sensitive && flagIsSet(flag, values);
    });
    if (!satisfied) found.push(`Give at least one of ${group.join(", ")}`);
  }
  return found;
}

/** The tier this particular run reaches: a row that reads until a flag escalates it stays a
 * read until the user sets one. A row that previews until it is applied always goes through
 * the preview. */
export function effectiveTier(row: CommandRow, values: FormValues): Tier {
  const tier = row.tier ?? "read";
  if (row.preview?.mode === "unless-applied") return tier;
  const base = row.baseTier ?? tier;
  if (base === tier) return tier;
  const escalated =
    row.flags.some((flag) => flag.escalates && flagIsSet(flag, values)) ||
    (row.operandEscalates && operandTokens(row, values).length > 0);
  return escalated ? tier : base;
}

export type RunPlan =
  | { kind: "terminal"; argv: string[] }
  | { kind: "run"; argv: string[]; ask: boolean }
  | { kind: "preview"; previewArgv: string[]; applyArgv: string[]; tier: Tier }
  | { kind: "direct"; argv: string[]; tier: Tier };

/** How the page runs the command: reads run at once, a writer with a dry run previews then
 * applies, a writer without one is confirmed and then runs, and a terminal-only command is
 * never run. A command that reads stdin is never previewed: the field is emptied when the
 * value is sent, so it could not be sent again for the apply. */
export function planRun(row: CommandRow, values: FormValues): RunPlan {
  const argv = baseArgv(row, values);
  if (isTerminal(row)) return { kind: "terminal", argv };
  const tier = effectiveTier(row, values);
  if (tier === "read") return { kind: "run", argv, ask: row.cost };
  const mode = stdinNeed(row) ? "none" : (row.preview?.mode ?? "none");
  const flag = managedFlag(row);
  if (mode === "flag" && flag)
    return { kind: "preview", previewArgv: [...argv, flag], applyArgv: argv, tier };
  if (mode === "unless-applied" && flag)
    return { kind: "preview", previewArgv: argv, applyArgv: [...argv, flag], tier };
  return { kind: "direct", argv, tier };
}

/** What the user types to confirm a destructive run: the first operand when it is short,
 * else the command. */
export function phraseFor(row: CommandRow, values: FormValues): string {
  const first = row.operands.find((operand) => operand.required);
  const text = first ? textOf(values.operands[first.name]) : "";
  return text !== "" && text.length <= MAX_PHRASE && !/\s/.test(text) ? text : row.id;
}

const SAFE = /^[A-Za-z0-9_@%+=:,./-]+$/;

export function shellQuote(word: string): string {
  if (SAFE.test(word)) return word;
  return `'${word.replace(/'/g, `'\\''`)}'`;
}

/** The line to type in a terminal. The app adds `--json` itself, so it is not shown. */
export function commandLine(argv: string[]): string {
  return ["toolportctl", ...argv.map(shellQuote)].join(" ");
}

export const toolCallArgv = (tool: string) => ["mcp", "call", tool, "--args-stdin"];

export type ToolPlan =
  | { kind: "run"; stdin: string }
  | { kind: "preview"; previewStdin: string; applyStdin: string; tier: Tier }
  | { kind: "direct"; stdin: string; tier: Tier };

/** The same safety rules for a self-management tool (contract section 15): the tool's own
 * `dry_run` previews, `confirm` (tier 3 and up) and `dry_run: false` apply. */
export function planTool(tool: ToolRow, args: Record<string, unknown>): ToolPlan {
  const tier = tool.tier ?? "read";
  const stdin = (value: Record<string, unknown>) => JSON.stringify(value);
  if (tier === "read") return { kind: "run", stdin: stdin(args) };
  const confirm = tool.toolTier >= 3 ? { confirm: true } : {};
  if (tool.dryRun === "none")
    return { kind: "direct", stdin: stdin({ ...args, ...confirm }), tier };
  return {
    kind: "preview",
    previewStdin: stdin({ ...args, dry_run: true }),
    applyStdin: stdin({ ...args, dry_run: false, ...confirm }),
    tier,
  };
}

export function parseToolArgs(text: string): Record<string, unknown> | string {
  const trimmed = text.trim();
  if (trimmed === "") return {};
  try {
    const value: unknown = JSON.parse(trimmed);
    if (value === null || typeof value !== "object" || Array.isArray(value))
      return "The arguments must be one JSON object";
    return value as Record<string, unknown>;
  } catch (error) {
    return `The arguments are not valid JSON: ${error instanceof Error ? error.message : "parse error"}`;
  }
}

/** Tools that no command covers: these are the ones the box has to offer. */
export function toolOnlyRows(data: CommandsData): ToolRow[] {
  return data.tools.filter((tool) => tool.command === null);
}

export function hasMcpCall(data: CommandsData): boolean {
  return data.commands.some((row) => row.id === "mcp call");
}
