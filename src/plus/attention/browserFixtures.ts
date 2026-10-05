import { actionArgvs } from "./fixtures";
import { DISMISS_CHOICES, dismissArgv, untilDate } from "./model";
import { createAttentionWorld } from "./world";

const world = createAttentionWorld();

const both = (argv: string[]) => [argv.join(" "), [...argv, "--dry-run"].join(" ")];

const dismissals = world.state.items.flatMap((item) =>
  DISMISS_CHOICES.flatMap(({ id }) => both(dismissArgv(item.id, untilDate(id)))),
);

/** What the dev browser fixture (`plusCtl.ts`) answers for the Attention screen and the
 * sidebar counter: the stateful Attention world, so the smoke walk sees a dismissal or an
 * action change the next read. One row per argv the screen runs: the list, the counter's read,
 * every dismissal with the dates the dialog offers, and every action with its dry-run twin. */
const argvs = [
  "attention ls",
  "attention ls --level needs-you",
  ...dismissals,
  ...actionArgvs(world.state.items).flatMap(both),
];

export const attentionBrowserFixtures: Array<[string, unknown]> = argvs.map(
  (argv): [string, () => unknown] => [argv, () => world.reply(argv.split(" "))],
);
