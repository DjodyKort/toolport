import type { ContextBundleLsData, ContextBundleShowData } from "../types/context-bundle";

export type BundleRow = ContextBundleLsData["bundles"][number];

export interface BundleForm {
  name: string;
  description: string;
  servers: string;
  skillsOff: string;
  skillsNameOnly: string;
  skillsAllow: string;
  pluginsOff: string;
  layersAdd: string;
  layersExclude: string;
  agentsOff: string;
  bind: string;
}

export const emptyBundleForm: BundleForm = {
  name: "",
  description: "",
  servers: "",
  skillsOff: "",
  skillsNameOnly: "",
  skillsAllow: "",
  pluginsOff: "",
  layersAdd: "",
  layersExclude: "",
  agentsOff: "",
  bind: "",
};

/** One value per line or comma; a glob never holds either. */
export function listOf(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((part) => part.trim())
    .filter(Boolean);
}

const join = (list: string[]) => list.join("\n");

export function formOf(show: ContextBundleShowData): BundleForm {
  return {
    name: show.name,
    description: show.description,
    servers: show.servers ?? "",
    skillsOff: join(show.skills.off),
    skillsNameOnly: join(show.skills.nameOnly),
    skillsAllow: join(show.skills.allow),
    pluginsOff: join(show.plugins.off),
    layersAdd: join(show.layers.add),
    layersExclude: join(show.layers.exclude),
    agentsOff: join(show.agents.off),
    bind: join(show.bind),
  };
}

const LISTS: Array<[keyof BundleForm, string]> = [
  ["skillsOff", "--skills-off"],
  ["skillsNameOnly", "--skills-name-only"],
  ["skillsAllow", "--skills-allow"],
  ["pluginsOff", "--plugins-off"],
  ["layersAdd", "--layers-add"],
  ["layersExclude", "--layers-exclude"],
  ["agentsOff", "--agents-off"],
  ["bind", "--bind"],
];

/** The flags of `bundle add` (every filled field) or `bundle edit` (the fields that differ
 * from `before`: each given list replaces the list of the definition). */
export function bundleFlags(form: BundleForm, before?: BundleForm): string[] {
  const flags: string[] = [];
  const changed = (key: keyof BundleForm) => !before || form[key] !== before[key];
  if (changed("description") && (before || form.description.trim()))
    flags.push("--description", form.description.trim());
  if (changed("servers") && (before || form.servers.trim()))
    flags.push("--servers", form.servers.trim());
  for (const [key, flag] of LISTS) {
    if (!changed(key)) continue;
    const list = listOf(form[key]);
    if (before || list.length > 0) flags.push(flag, list.join(","));
  }
  return flags;
}

export const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

export function nameProblem(name: string, taken: string[]): string | null {
  if (!name) return null;
  if (!NAME.test(name)) return "Letters, digits, dot, dash and underscore.";
  return taken.includes(name) ? "A profile with this name exists." : null;
}

/** What a bundle does, one line per part, for the list and the detail. */
export function bundleParts(row: {
  skills: { off: number; nameOnly: number; allow: number };
  plugins: { off: string[] };
  layers: { add: string[]; exclude: string[] };
  agents: { off: string[] };
}): string[] {
  const parts: string[] = [];
  const { off, nameOnly, allow } = row.skills;
  if (off + nameOnly + allow > 0)
    parts.push(
      allow > 0
        ? `only ${allow} skill${allow === 1 ? "" : "s"}`
        : `${off + nameOnly} skill${off + nameOnly === 1 ? "" : "s"} changed`,
    );
  if (row.plugins.off.length > 0) parts.push(`${row.plugins.off.length} plugin(s) off`);
  if (row.layers.add.length + row.layers.exclude.length > 0) parts.push("layers");
  if (row.agents.off.length > 0) parts.push(`${row.agents.off.length} agent(s) off`);
  return parts;
}

/** `context use` applies the server set and the bundle under one name; a bundle without a
 * paired server set goes through `context bundle apply`. */
export function applyArgv(row: { name: string; servers: string | null }, cwd: string) {
  return row.servers
    ? ["context", "use", row.name, "--cwd", cwd]
    : ["context", "bundle", "apply", row.name, "--cwd", cwd];
}

export const applyCommand = (row: { servers: string | null }) =>
  row.servers ? "context use" : "context bundle apply";
