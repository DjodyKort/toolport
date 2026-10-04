import { goldenReply, systemCtlFixtures, updateWorld } from "./fixtures";

/** What the dev browser fixture answers for the System screen: the static world of
 * `fixtures.ts` (the command list comes from the shared fixture), the update list with every
 * state, and the importer for the folder `/old/mcpm` and the tools file `/old/tools.json`. */
export const systemBrowserFixtures: Array<[string, unknown]> = [
  ...systemCtlFixtures.filter(([key]) => key !== "commands"),
  ["update --check", updateWorld],
  ["import mcpm /old/mcpm --dry-run", goldenReply("import-mcpm.preview")],
  ["import mcpm /old/mcpm", goldenReply("import-mcpm.apply")],
  [
    "import mcpm /old/mcpm --dry-run --tools /old/tools.json --name-map",
    goldenReply("import-mcpm.name-map"),
  ],
];
