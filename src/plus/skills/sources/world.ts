/** A stateful version of the Sources fixtures, shaped like the goldens of `sources ls` and
 * `sources root`: an applied `sources root add|rm` changes what the next `sources root ls`
 * answers, a preview never does. Browser safe: no node imports. The folders are fixed because
 * a fixture row is keyed by its argv. */
import type { SourcesRootLsData } from "../../bridge/data";
import {
  plusSourcesFixture,
  plusSourcesItemsFixture,
  plusSourcesRootFixture,
} from "../../fixtures/sources";

export const ADD_ROOT = "/home/demo/work/client-repo";
export const REMOVE_ROOT = "/home/demo/dups";
const CONFIG = "/home/demo/.config/toolport/context.json";
const BACKUP = "/home/demo/.cache/toolport/context/backups/20261004-100000/context.json";

type Roots = SourcesRootLsData["roots"];

const shorthand = (roots: Roots) =>
  JSON.stringify(
    roots
      .filter((root) => root.origin === "config")
      .map((root) => root.path.replace("/home/demo", "~")),
  );

export function createSourcesWorld(): Array<[string, () => unknown]> {
  let roots: Roots = plusSourcesRootFixture.roots.map((root) => ({ ...root }));
  const rows = new Map<string, () => unknown>();
  const on = (argv: string, reply: () => unknown) => rows.set(argv, reply);

  on("sources ls", () => plusSourcesFixture);
  on("sources ls --refresh", () => plusSourcesFixture);
  for (const row of plusSourcesFixture.sources)
    on(`sources ls --source ${row.id} --items`, () => ({
      ...plusSourcesItemsFixture,
      sources: [row],
      items: (plusSourcesItemsFixture.items ?? []).filter((i) => i.sourceId === row.id),
    }));
  on("sources root ls", () => ({ roots }));

  const change = (path: string, add: boolean, dryRun: boolean) => {
    const after = add
      ? [...roots, { path, origin: "config" as const, exists: true, repo: true }]
      : roots.filter((root) => root.path !== path);
    const plan = {
      summary: `${add ? "add" : "remove"} source root ${path}`,
      steps: [
        {
          op: "merge" as const,
          path: CONFIG,
          detail: `${add ? "add" : "remove"} ${path} ${add ? "to" : "from"} sourceRoots`,
          keys: ["sourceRoots"],
          diff: { before: shorthand(roots), after: shorthand(after) },
        },
      ],
      effects: {},
      warnings: [] as string[],
      undo: `toolportctl sources root ${add ? "rm" : "add"} ${path}`,
    };
    if (!dryRun) roots = after;
    return {
      dryRun,
      plan,
      result: dryRun
        ? null
        : {
            applied: true as const,
            changed: [CONFIG],
            undo: plan.undo,
            backups: [BACKUP],
          },
    };
  };
  for (const [path, add] of [
    [ADD_ROOT, true],
    [REMOVE_ROOT, false],
  ] as const) {
    const verb = add ? "add" : "rm";
    on(`sources root ${verb} ${path} --dry-run`, () => change(path, add, true));
    on(`sources root ${verb} ${path}`, () => change(path, add, false));
  }
  return [...rows];
}
