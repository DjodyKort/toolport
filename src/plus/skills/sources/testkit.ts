import type { SourcesLsData, SourcesRootLsData } from "../../bridge/data";
import { createBridge, goldenData, type Bridge } from "../testkit";

export { goldenData, failure, wire, type Bridge } from "../testkit";

/** The goldens scrub the fixture home to `<WORLD>`; a test reads it as `/fixture`. */
const unscrub = <T>(data: T): T =>
  JSON.parse(JSON.stringify(data).split("<WORLD>").join("/fixture")) as T;

export const sourcesData = (): SourcesLsData => unscrub(goldenData("sources-ls.items"));

export const summaryData = (): SourcesLsData => {
  const { generatedAt, partial, skipped, sources } = sourcesData();
  return { generatedAt, partial, skipped, sources };
};

/** One source with its items, as `sources ls --source <id> --items` answers it. */
export function itemsOf(id: string): SourcesLsData {
  const all = sourcesData();
  return {
    ...all,
    sources: all.sources.filter((row) => row.id === id),
    items: (all.items ?? []).filter((item) => item.sourceId === id),
  };
}

export const rootsData = (): SourcesRootLsData => unscrub(goldenData("sources-root-ls"));

export const ADDED = "/fixture/home/work";
export const REMOVED = "/fixture/home/dups";

/** The Skills bridge plus every argv of the Sources tab, answered from the real goldens. Adding
 * or removing a root changes what the next `sources root ls` returns, like the real command. */
export function createSourcesBridge(): Bridge {
  const bridge = createBridge();
  let roots = rootsData().roots;
  bridge.set("sources ls", () => summaryData());
  bridge.set("sources ls --refresh", () => summaryData());
  for (const row of sourcesData().sources)
    bridge.set(`sources ls --source ${row.id} --items`, () => itemsOf(row.id));
  bridge.set("sources root ls", () => ({ roots }));
  bridge.set(`sources root add ${ADDED} --dry-run`, () =>
    unscrub(goldenData("sources-root-add.preview")),
  );
  bridge.set(`sources root add ${ADDED}`, () => {
    roots = [...roots, { path: ADDED, origin: "config", exists: true, repo: false }];
    return unscrub(goldenData("sources-root-add.apply"));
  });
  bridge.set(`sources root rm ${REMOVED} --dry-run`, () =>
    unscrub(goldenData("sources-root-rm.preview")),
  );
  bridge.set(`sources root rm ${REMOVED}`, () => {
    roots = roots.filter((root) => root.path !== REMOVED);
    return unscrub(goldenData("sources-root-rm.apply"));
  });
  return bridge;
}
