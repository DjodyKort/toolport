import { plusSourcesFixture } from "../fixtures/sources";
import { createContextWorld } from "./world";

/** What the dev browser fixture (`plusCtl.ts`) answers for the Context screen: the stateful
 * context world, so the smoke walk sees a sync, a profile add or a disable change the next read.
 * The Layers tab also reads the org file from the sources list. */
export const contextBrowserFixtures: Array<[string, () => unknown]> = [
  ...createContextWorld().rows(),
  [
    "sources ls --source org",
    () => ({
      ...plusSourcesFixture,
      sources: plusSourcesFixture.sources.filter((source) => source.id === "org"),
    }),
  ],
];
