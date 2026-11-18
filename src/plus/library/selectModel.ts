import type {
  ItemKind,
  LineCount,
  PlanRowData,
  SelectItem,
  SetChanges,
} from "./selectData";

export const KINDS: ItemKind[] = ["skill", "agent", "command", "hook", "rule"];

export const KIND_TITLE: Record<ItemKind, string> = {
  skill: "Skills",
  agent: "Agents",
  command: "Commands",
  hook: "Hooks",
  rule: "Rules",
};

export const GROUP_LIMIT = 40;

export type Want = "on" | "off" | "tick";
export type Staged = Record<string, Want>;
export type StateView = "all" | "on" | "new";

export const itemRef = (item: { kind: ItemKind; id: string }) =>
  `${item.kind}:${item.id}`;

export const shortRef = (sha: string | null | undefined) => (sha ? sha.slice(0, 7) : "");

/** The flags take a comma list; `\,` is a comma that belongs to the name. */
export const escapeCommas = (ref: string) =>
  ref.replace(/\\/g, "\\\\").replace(/,/g, "\\,");

const storedOn = (item: SelectItem) => item.state === "on" || item.state === "removed";

export function isOn(item: SelectItem, staged: Staged): boolean {
  const want = staged[itemRef(item)];
  if (want) return want !== "off";
  return storedOn(item);
}

function matchesStored(item: SelectItem, want: Want): boolean {
  if (want === "tick") return item.state === "on" && !item.pending;
  if (want === "on") return storedOn(item);
  return !storedOn(item);
}

/** Records a choice, and forgets it again when it is what is stored already. */
export function stage(staged: Staged, item: SelectItem, want: Want): Staged {
  const next = { ...staged };
  const ref = itemRef(item);
  if (matchesStored(item, want)) delete next[ref];
  else next[ref] = want;
  return next;
}

export function changesOf(staged: Staged): SetChanges {
  const refs = (want: Want) =>
    Object.entries(staged)
      .filter(([, value]) => value === want)
      .map(([ref]) => ref)
      .sort();
  return { on: refs("on"), off: refs("off"), tick: refs("tick") };
}

export const changeCount = (staged: Staged) => Object.keys(staged).length;

export const onCount = (items: SelectItem[], staged: Staged) =>
  items.filter((item) => isOn(item, staged)).length;

/** Chosen in the store but not yet part of the generated plugin, or the reverse. */
export const unapplied = (items: SelectItem[]) =>
  items.some(
    (item) =>
      item.kind !== "rule" &&
      ((storedOn(item) && !item.inSlice && !item.pending) ||
        (!storedOn(item) && item.inSlice)),
  );

export interface Filter {
  query: string;
  kind: ItemKind | "all";
  view: StateView;
}

export function visibleItems(
  items: SelectItem[],
  staged: Staged,
  filter: Filter,
): SelectItem[] {
  const query = filter.query.trim().toLowerCase();
  return items.filter((item) => {
    if (filter.kind !== "all" && item.kind !== filter.kind) return false;
    if (filter.view === "on" && !isOn(item, staged)) return false;
    if (filter.view === "new" && item.state !== "new") return false;
    if (!query) return true;
    return [item.id, item.kind, item.command ?? "", item.event ?? ""].some((text) =>
      text.toLowerCase().includes(query),
    );
  });
}

export function grouped(items: SelectItem[]): Array<[ItemKind, SelectItem[]]> {
  return KINDS.map((kind): [ItemKind, SelectItem[]] => [
    kind,
    items.filter((item) => item.kind === kind),
  ]).filter(([, rows]) => rows.length > 0);
}

export function hookTitle(item: SelectItem): string {
  return item.matcher ? `${item.event} ${item.matcher}` : (item.event ?? item.id);
}

/** The script a hook command runs from the source, when it names one. */
export function hookScript(command: string | undefined): string | null {
  const found = /\$\{CLAUDE_PLUGIN_ROOT\}"?\/([^\s"']+)/.exec(command ?? "");
  return found?.[1] ?? null;
}

export function lineSummary(count: LineCount | undefined): string {
  if (!count) return "content changed";
  return `content changed (+${count.added} / −${count.removed} lines)`;
}

export const rowLabel = (row: PlanRowData) => `${row.kind}:${row.id}`;

const SAFE = /^[A-Za-z0-9_:./@%+=,-]+$/;
export const quote = (text: string) =>
  SAFE.test(text) ? text : `'${text.replace(/'/g, `'\\''`)}'`;

export function setCommand(source: string, changes: SetChanges): string | null {
  const parts = [
    ...changes.on.map((ref) => `--on ${quote(escapeCommas(ref))}`),
    ...changes.off.map((ref) => `--off ${quote(escapeCommas(ref))}`),
    ...changes.tick.map((ref) => `--tick-hook ${quote(escapeCommas(ref))}`),
  ];
  if (parts.length === 0) return null;
  return `toolportctl library select set ${source} ${parts.join(" ")}`;
}

export function commandLines(
  source: string,
  changes: SetChanges,
  hash: string | null,
  switchWhole: boolean,
): string[] {
  const flag = switchWhole ? " --switch" : "";
  return [
    setCommand(source, changes),
    `toolportctl library select plan ${source}${flag}`,
    hash
      ? `toolportctl library select apply ${source} --plan-hash ${hash}${flag}`
      : `toolportctl library select apply ${source} --plan-hash <hash>${flag}`,
    `toolportctl library select undo ${source}`,
  ].filter((line): line is string => line !== null);
}
