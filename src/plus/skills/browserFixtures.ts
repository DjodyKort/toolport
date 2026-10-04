import { skillsBrowserFixtures as library } from "./fixtures";
import { tapsBrowserFixtures } from "./fixturesTaps";

/** What the dev browser fixture (`plusCtl.ts`) answers for the Library screen. */
export const skillsBrowserFixtures: Array<[string, unknown]> = [
  ...library,
  ...tapsBrowserFixtures,
];
