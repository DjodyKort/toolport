import { TOOL_NAMES, createToolsWorld } from "./world";

const world = createToolsWorld();

/** What the dev browser fixture (`plusCtl.ts`) answers for the servers tools: the stateful
 * tools world, so the smoke walk sees an update or a sync change the next read. The browser
 * fixture carries no stdin, so a call is answered as if it asked about the first server. */
export const serversToolsBrowserFixtures: Array<[string, unknown]> = TOOL_NAMES.map(
  (tool): [string, () => unknown] => {
    const argv = ["mcp", "call", tool, "--args-stdin"];
    return [argv.join(" "), () => world.reply(argv, undefined)];
  },
);
