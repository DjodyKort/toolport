import type { AttentionData, AttentionRow } from "../bridge/data";
import type { CtlReply } from "./ctlWorld";

const MIN = 60_000;
const HOUR = 60 * MIN;

export type AttentionScenario = "busy" | "clear" | "unchecked";

export interface AttentionWorldOptions {
  /** Without a scenario `attention ls` fails the way a build without toolportctl does, so the bell
   * stays out. */
  scenario?: AttentionScenario;
}

const iso = (at: number) => new Date(at).toISOString().replace(/\.\d+Z$/, "Z");

function busy(now: number): AttentionRow[] {
  return [
    {
      id: "tasks:waiting:run-export-notes",
      urgency: "needs-you",
      title: "Task export-notes is waiting for you",
      detail: "Stopped at step 3 of 4: confirm upload.",
      source: "Tasks",
      since: iso(now - 25 * MIN),
      action: { label: "Continue", screen: "tasks" },
    },
    {
      id: "credentials:secret:alpha-api",
      urgency: "needs-you",
      title: "alpha-api is missing a secret",
      detail: "API_KEY is not set, so the server cannot start.",
      source: "Credentials",
      action: { label: "Set secret", screen: "servers" },
    },
    {
      id: "credentials:login:beta-mail",
      urgency: "needs-you",
      title: "beta-mail needs a new sign-in",
      detail: "The login stopped working. Check it again, or sign in once more.",
      source: "Credentials",
      since: iso(now - 3 * HOUR),
      action: { label: "Sign in", screen: "servers" },
    },
    {
      id: "tasks:failed:nightly-sync",
      urgency: "worth-a-look",
      title: "Task nightly-sync failed",
      detail: "Step 2 of 3 (update-plugins) ended with an error.",
      source: "Tasks",
      since: iso(now - 6 * HOUR),
      action: { label: "Run again", screen: "tasks" },
    },
    {
      id: "library:behind",
      urgency: "worth-a-look",
      title: "The library is 2 commits behind its remote",
      detail: "A pull would fast-forward it.",
      source: "Library",
      action: { label: "Review", screen: "library" },
    },
  ];
}

const CHECKS = ["credentials", "tasks", "library"];

/** The answers of `toolportctl attention ls` for the fixture app. */
export function createAttentionWorld(options: AttentionWorldOptions = {}) {
  return {
    run(argv: string[]): CtlReply {
      if (argv[1] !== "ls") {
        return {
          ok: false,
          error: { code: "usage", message: `unknown command: ${argv.join(" ")}` },
        };
      }
      if (!options.scenario) {
        return {
          ok: false,
          error: {
            code: "unavailable",
            message: "attention is not part of this fixture",
          },
        };
      }
      const now = Date.now();
      const items = options.scenario === "clear" ? [] : busy(now);
      const unchecked = options.scenario === "unchecked";
      const data: AttentionData = {
        checkedAt: iso(now - 2 * MIN),
        counts: {
          needsYou: items.filter((i) => i.urgency === "needs-you").length,
          worthALook: items.filter((i) => i.urgency === "worth-a-look").length,
        },
        checks: CHECKS.map((name) =>
          unchecked && name === "tasks"
            ? { name, ok: false, error: "the data directory could not be resolved" }
            : { name, ok: true },
        ),
        items,
      };
      return { ok: true, data };
    },
  };
}
