import { createContextWorld } from "./world";

/** What the dev browser fixture (`plusCtl.ts`) answers for the Context screen: the stateful
 * context world, so the smoke walk sees a sync, a profile add or a disable change the next read. */
export const contextBrowserFixtures: Array<[string, () => unknown]> =
  createContextWorld().rows();
