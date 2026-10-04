import { systemCtlFixtures } from "./fixtures";
import { COUNCIL_KEY, createSystemWorld } from "./world";

const ROOT = "/old/mcpm";
const TOOLS = "/old/tools.json";
export const WALK = {
  repo: "git@git.example.com:me/toolport-sync.git",
  machine: "work-laptop",
};

const world = createSystemWorld();

const both = (argv: string) => [argv, `${argv} --dry-run`];

/** What the dev browser fixture (`plusCtl.ts`) answers for the System screen: the stateful
 * System world, so the smoke walk sees an init, a push or an update change the next read. One
 * row per argv the tabs run with the forms the walk fills in; the browser fixture does not
 * carry stdin, so a passphrase or key command is answered as if one had been given. */
const argvs = [
  "sync status",
  "sync diff",
  "sync git-sync --status",
  `sync init --repo ${WALK.repo} --machine-id ${WALK.machine} --passphrase-stdin`,
  ...both("sync push"),
  ...both("sync pull"),
  "update --check",
  "update srv-git --check",
  ...both("update srv-git --apply"),
  ...both("update --apply"),
  ...both("update --init"),
  "council doctor",
  "council tools",
  "council install",
  "council uninstall",
  `secret set council ${COUNCIL_KEY}`,
  "mcp doctor",
  "mcp tools",
  "mcp install",
  "mcp uninstall",
  ...both(`import mcpm ${ROOT}`),
  `import mcpm ${ROOT} --dry-run --tools ${TOOLS} --name-map`,
];

export const systemBrowserFixtures: Array<[string, unknown]> = [
  ...systemCtlFixtures.filter(([key]) => key !== "commands" && !argvs.includes(key)),
  ...[...new Set(argvs)].map((argv): [string, () => unknown] => [
    argv,
    () => world.reply(argv.split(" "), "stdin"),
  ]),
];
