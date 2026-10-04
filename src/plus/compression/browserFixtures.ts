import { createCompressionWorld, PROVIDER_IDS } from "./world";

const FOLDER = "/fixture/home/project";
const PRESET_NAMES = ["interactive", "agent", "balanced"];

const world = createCompressionWorld();

const both = (argv: string) => [argv, `${argv} --dry-run`];

/** What the dev browser fixture (`plusCtl.ts`) answers for the Compression tab: the stateful
 * compression world, so the smoke walk sees a provider switch, an install or a recorded
 * entry change the next read. One row per argv the tab can run with its default forms. */
const argvs = [
  "compression status",
  "compression presets",
  "compression pin",
  "compression doctor",
  "compression ledger summary",
  "compression verify",
  "compression seal --dry-run",
  "compression seal --apply",
  "compression pin --install",
  "compression pin --refresh",
  "compression pin 0.30.0",
  "compression presets --refresh",
  "compression update --latest",
  "compression update --latest --accept",
  "compression update --to 0.30.0",
  "compression update --to 0.30.0 --accept",
  "compression proxy up",
  "compression proxy down",
  "compression proxy restart",
  "compression ledger record --provider rtk-only --before 1000 --after 400",
  "compression ledger record --provider headroom --before 20000 --after 8000",
  `compression run --plan --cwd ${FOLDER} claude`,
  `compression env --cwd ${FOLDER}`,
  ...PROVIDER_IDS.flatMap((id) => both(`compression set-provider ${id}`)),
  ...PRESET_NAMES.flatMap((name) => both(`compression use ${name}`)),
  ...PROVIDER_IDS.filter((id) => id !== "none").flatMap((id) =>
    both(`compression enable --provider ${id}`),
  ),
  ...both("compression disable"),
  ...both("compression disable --teardown"),
  ...both("compression sync"),
  "compression pin --install --dry-run",
  "compression pin --refresh --dry-run",
  "compression pin 0.30.0 --dry-run",
  "compression presets --refresh --dry-run",
];

export const compressionBrowserFixtures: Array<[string, () => unknown]> = [
  ...new Set(argvs),
].map((argv) => [argv, () => world.reply(argv.split(" "))]);
