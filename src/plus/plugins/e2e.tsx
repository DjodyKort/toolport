import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CtlResult } from "../bridge/ctl";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { PlusViews } from "../PlusViews";
import { createMcpWorld } from "../agents/mcpWorld";
import { createContextWorld } from "../context/world";
import { createSkillsWorld } from "../skills/world";
import { createSystemWorld } from "../system/world";
import { createPluginsWorld, type PluginsState } from "./world";

export const FOLDER = "/home/demo/work/acme-erp";
export const SENTINEL = "SENTINEL-NOT-FOR-THE-DOM";

const registry = JSON.parse(
  readFileSync(
    join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json"),
    "utf8",
  ),
).envelope.data;

type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

const isFailure = (value: unknown): value is CtlReplyFailure =>
  typeof value === "object" &&
  value !== null &&
  "code" in value &&
  "message" in value &&
  !("plan" in value);

/** A fake `plus_ctl` bridge for the plugin and hook screens in the three places they live
 * (Library, Context, System): the stateful plugins world answers `plugins`, `hooks` and `cc`,
 * and the worlds of the neighbouring tabs answer what those screens read besides. A test
 * overrides one argv with `set`; an argv nobody answers is recorded in `missing`. */
export function createBridge(seed: Partial<PluginsState> = {}) {
  const world = createPluginsWorld(seed);
  const skills = createSkillsWorld({});
  const context = createContextWorld({});
  const system = createSystemWorld({});
  const mcp = new Map(createMcpWorld());
  const replies = new Map<string, Reply>([["commands", registry]]);
  const calls: Array<{ job: string; argv: string[] }> = [];
  const missing: string[] = [];
  const jobs = new Map<string, string[]>();
  let count = 0;

  const answer = (argv: string[]): unknown => {
    const key = argv.join(" ");
    if (replies.has(key)) {
      const wanted = replies.get(key);
      return typeof wanted === "function" ? wanted(argv) : wanted;
    }
    const own = world.reply(argv);
    if (own !== undefined) return own;
    const tool = mcp.get(key);
    if (tool) return tool(argv);
    if (argv[0] === "context") return context.run(argv, null);
    const row = skills.get(key);
    return row ? row() : system.reply(argv, null);
  };

  return {
    calls,
    missing,
    world,
    set(argv: string, reply: Reply) {
      replies.set(argv, reply);
    },
    ran: () => calls.map((call) => call.argv.join(" ")),
    count: (argv: string) => calls.filter((call) => call.argv.join(" ") === argv).length,
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call = { job: `job-${++count}`, argv: args.argv as string[] };
        calls.push(call);
        jobs.set(call.job, call.argv);
        return call.job;
      }
      if (command === "plus_ctl_result") {
        const argv = jobs.get(args.job as string);
        if (!argv) throw new Error(`unknown job ${String(args.job)}`);
        const value = await answer(argv);
        if (value === undefined) {
          missing.push(argv.join(" "));
          throw new Error(`no fake reply for plus_ctl ${argv.join(" ")}`);
        }
        const failed = isFailure(value) ? value : null;
        return {
          job: args.job as string,
          exitCode: failed ? 1 : 0,
          signal: null,
          cancelled: false,
          parseError: null,
          stderr: [],
          truncated: false,
          envelope: failed
            ? {
                ok: false,
                command: argv[0],
                schemaVersion: 1,
                data: failed.data,
                error: { code: failed.code, message: failed.message },
              }
            : { ok: true, command: argv[0], schemaVersion: 1, data: value },
        } satisfies CtlResult;
      }
      if (command === "plus_ctl_cancel") return null;
      throw new Error(`unexpected invoke ${command}`);
    },
  };
}

export type Bridge = ReturnType<typeof createBridge>;

export { CtlReplyFailure };

export async function openView(
  view: "library" | "context" | "system",
  tab: string,
  ready: () => Promise<unknown> = async () => {},
) {
  const user = userEvent.setup();
  render(<PlusViews view={view} onSelectView={() => {}} />);
  const tabs = await screen.findByRole("tablist", {
    name: `${view[0].toUpperCase()}${view.slice(1)} sections`,
  });
  await user.click(within(tabs).getByRole("tab", { name: tab }));
  await ready();
  return user;
}

export const openPlugins = () =>
  openView("library", "Plugins", () =>
    screen.findByRole("region", { name: "Plugin ecc" }),
  );

export const openHooks = () =>
  openView("context", "Hooks", () => screen.findByRole("group", { name: "Hook counts" }));

export const openUpdates = () =>
  openView("system", "Updates", () =>
    screen.findByRole("list", { name: "Plugin updates" }),
  );

/** Types the folder into the field of the open tab and shows it. */
export async function chooseFolder(
  user: ReturnType<typeof userEvent.setup>,
  button: string,
  folder = FOLDER,
) {
  await user.clear(screen.getByLabelText("Folder"));
  await user.type(screen.getByLabelText("Folder"), folder);
  await user.click(screen.getByRole("button", { name: button }));
  await waitFor(() => expect(screen.getByLabelText("Folder")).toHaveValue(folder));
}

/** The page text and every field value: nothing sensitive may be in it. */
export const pageText = () =>
  document.body.innerHTML +
  [...document.querySelectorAll("input,textarea")]
    .map((el) => (el as HTMLInputElement).value)
    .join("|");
