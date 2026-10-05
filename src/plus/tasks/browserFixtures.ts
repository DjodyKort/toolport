import { loginTasks, stockTasks } from "./fixtures";
import { createTasksWorld } from "./world";

const world = createTasksWorld({
  tasks: [
    ...stockTasks,
    ...loginTasks.tasks.map((row) => ({
      ...stockTasks[1],
      id: row.id,
      title: row.title,
      triggers: row.triggers,
    })),
  ],
});

const both = (argv: string) => [argv, `${argv} --dry-run`];

/** What the dev browser fixture (`plusCtl.ts`) answers for the Tasks screen and the Refresh
 * task action of Logins: the stateful Tasks world, so the smoke walk sees a run advance and a
 * write change the next read. One row per argv the screen runs; the browser fixture carries no
 * stdin, so a saved definition is answered as if the form had sent a valid one. */
const argvs = [
  "task ls",
  "task ls --all",
  "task history --limit 50",
  "task show portal-token",
  "task show nightly-report",
  "task show draft-cleanup",
  "task history --run run-fixture-ok",
  "task history --run run-fixture-waiting",
  ...both("task run portal-token"),
  ...both("task run nightly-report"),
  "task resume run-fixture-waiting",
  "task cancel run-fixture-waiting",
  ...both("task rm draft-cleanup"),
];

export const tasksBrowserFixtures: Array<[string, unknown]> = argvs.map(
  (argv): [string, () => unknown] => [
    argv,
    () => world.reply(argv.split(" "), JSON.stringify(stockTasks[1])),
  ],
);
