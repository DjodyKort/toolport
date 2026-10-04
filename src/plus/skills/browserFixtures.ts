import { createSkillsWorld } from "./world";

/** What the dev browser fixture (`plusCtl.ts`) answers for the Library screen: the stateful
 * skills world, so the smoke walk sees a sync, a clean or an install change the next read. */
export const skillsBrowserFixtures: Array<[string, () => unknown]> = [
  ...createSkillsWorld({ browser: true }),
];
