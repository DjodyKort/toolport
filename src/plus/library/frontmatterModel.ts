export type FieldValue =
  string | number | boolean | null | unknown[] | Record<string, unknown>;

/** How the editor treats a field: text, yes/no and numbers can be changed in the app; a list or a
 * mapping (hooks, metadata, dependencies) stays in the editor, and `name` is the folder's. */
export type FieldKind = "text" | "boolean" | "number" | "locked" | "complex";

export const LISTING_KEYS = ["description", "when_to_use"];

/** Hyphens and underscores are the same key to Claude Code's frontmatter and to Toolport. */
export const norm = (key: string) => key.replace(/-/g, "_");

export function kindOf(key: string, value: unknown): FieldKind {
  if (norm(key) === "name") return "locked";
  if (typeof value === "string") return "text";
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return "number";
  return "complex";
}

export function lookup(
  fields: Record<string, unknown>,
  key: string,
): [string, unknown] | null {
  const hit = Object.entries(fields).find(([k]) => norm(k) === norm(key));
  return hit ?? null;
}

export interface Draft {
  /** The key as the file spells it; for an added field, as typed. */
  key: string;
  kind: FieldKind;
  /** What the input holds: text, "true" or "false", or the digits. */
  text: string;
  removed: boolean;
  added: boolean;
}

export function draftsOf(fields: Record<string, unknown>): Draft[] {
  return Object.entries(fields).map(([key, value]) => {
    const kind = kindOf(key, value);
    return {
      key,
      kind,
      text:
        kind === "complex"
          ? JSON.stringify(value)
          : typeof value === "string"
            ? value
            : String(value),
      removed: false,
      added: false,
    };
  });
}

export function valueOfDraft(
  draft: Draft,
): { ok: true; value: FieldValue } | { ok: false; why: string } {
  if (draft.kind === "boolean") return { ok: true, value: draft.text === "true" };
  if (draft.kind === "number") {
    const n = Number(draft.text);
    return draft.text.trim() !== "" && Number.isFinite(n)
      ? { ok: true, value: n }
      : { ok: false, why: `${draft.key} must be a number` };
  }
  return { ok: true, value: draft.text };
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

export interface PatchResult {
  patch: Record<string, FieldValue>;
  /** The fields that cannot be sent, by what is wrong with them. */
  problems: string[];
}

/** What the drafts change against the fields as they were read: a new value, `null` for a removed
 * field, nothing for a field left alone. */
export function patchOf(fields: Record<string, unknown>, drafts: Draft[]): PatchResult {
  const patch: Record<string, FieldValue> = {};
  const problems: string[] = [];
  for (const draft of drafts) {
    if (draft.kind === "locked" || draft.kind === "complex") continue;
    const existing = lookup(fields, draft.key);
    if (draft.removed) {
      if (existing) patch[draft.key] = null;
      continue;
    }
    const value = valueOfDraft(draft);
    if (!value.ok) {
      problems.push(value.why);
      continue;
    }
    if (!existing || !same(existing[1], value.value)) patch[draft.key] = value.value;
  }
  return { patch, problems };
}

/** The patch that puts the fields back as they were: the old value, or `null` for a field the edit
 * added. Only the keys of `patch` are touched. */
export function undoPatchOf(
  before: Record<string, unknown>,
  patch: Record<string, FieldValue>,
): Record<string, FieldValue> {
  const back: Record<string, FieldValue> = {};
  for (const key of Object.keys(patch)) {
    const old = lookup(before, key);
    back[key] = old ? (old[1] as FieldValue) : null;
  }
  return back;
}

/** True when every key of `patch` has, in `fields`, the value the patch gave it (`null` meaning
 * the field is gone). Undo uses it to see that nothing changed since the edit. */
export function stillAsWritten(
  fields: Record<string, unknown>,
  patch: Record<string, FieldValue>,
): boolean {
  return Object.entries(patch).every(([key, value]) => {
    const found = lookup(fields, key);
    return value === null ? found === null : found !== null && same(found[1], value);
  });
}

/** The characters Claude Code counts: the description and `when_to_use` together. */
export function listingLength(drafts: Draft[]): {
  description: number;
  whenToUse: number;
} {
  const chars = (key: string) => {
    const draft = drafts.find((d) => norm(d.key) === key);
    return draft && !draft.removed && draft.kind === "text" ? [...draft.text].length : 0;
  };
  return { description: chars("description"), whenToUse: chars("when_to_use") };
}
