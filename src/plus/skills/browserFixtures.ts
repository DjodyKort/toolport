import { createSkillsWorld } from "./world";

/** What the dev browser fixture (`plusCtl.ts`) answers for the Library screen: the stateful
 * skills world, so the smoke walk sees a sync, a clean or an install change the next read. The
 * library clone starts two commits behind its remote with a leaked secret waiting to be pushed:
 * the pull goes through, the push is blocked by the scan. */
export const skillsBrowserFixtures: Array<[string, () => unknown]> = [
  ...createSkillsWorld({ browser: true, library: { ahead: 2, leak: true } }),
];
