import { createPluginsWorld } from "./world";

const world = createPluginsWorld();

export const WALK_FOLDER = "/home/demo/work/acme-erp";
const SET = "--set hook_profile=minimal --set gateguard=off";
const both = (argv: string) => [argv, `${argv} --dry-run`];

/** What the dev browser fixture (`plusCtl.ts`) answers for Library > Plugins, Context > Hooks
 * and the System > Updates plugin card: the stateful plugins world, so the smoke walk sees an
 * applied settings write, a server deny, a turn-off or an update change the next read. One row
 * per argv the screens run with the folder and the settings the walk uses, with and without
 * `--dry-run`. */
const argvs = [
  "plugins ls",
  `plugins ls --cwd ${WALK_FOLDER}`,
  ...["ecc@ecc", "demo-plugin@fake-market"].flatMap((id) => [
    `plugins show ${id}`,
    `plugins show ${id} --cwd ${WALK_FOLDER}`,
  ]),
  "hooks ls",
  `hooks ls --cwd ${WALK_FOLDER}`,
  "cc list",
  ...both("cc update"),
  ...both("cc update ecc"),
  ...both(`plugins config ecc@ecc --cwd ${WALK_FOLDER} ${SET}`),
  ...both(
    `plugins config ecc@ecc --cwd ${WALK_FOLDER} --unset hook_profile --unset gateguard`,
  ),
  ...["deny", "allow"].flatMap((action) =>
    both(`plugins mcp ${action} ecc@ecc chrome-devtools --cwd ${WALK_FOLDER}`),
  ),
  ...["off", "on"].flatMap((action) =>
    both(`plugins ${action} ecc@ecc --cwd ${WALK_FOLDER}`),
  ),
  ...["disable", "enable"].flatMap((action) => both(`plugins ${action} ecc@ecc`)),
];

export const pluginsBrowserFixtures: Array<[string, () => unknown]> = argvs.map(
  (argv) => [argv, () => world.reply(argv.split(" "))],
);
