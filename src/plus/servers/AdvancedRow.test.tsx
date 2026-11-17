import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import {
  setsFromIds,
  stdioServer,
  type RestartScript,
  type WorldServer,
} from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import ServerOperations from "./ServerOperations";
import { resetRestartStore } from "./restartStore";
import { entryOf, getRegistryMock, invokeLog, ranArgv, registryOf } from "./testkit";
import { resetServerWatch } from "./watch";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("./testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  getRegistry: (...args: unknown[]) => getRegistryMock(...args),
}));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

interface Options {
  server?: Partial<WorldServer>;
  others?: WorldServer[];
  running?: boolean;
  restart?: RestartScript;
}

function setup(options: Options = {}) {
  const server = stdioServer("srv-bravo", "bravo-build", options.server);
  const all = [server, ...(options.others ?? [])];
  const sets = setsFromIds(["local"], all);
  resetPlusFixture({
    servers: {
      servers: all,
      sets,
      restart: options.restart ? { "srv-bravo": options.restart } : {},
    },
  });
  const registry = registryOf(all, sets);
  getRegistryMock.mockResolvedValue(registry);
  render(
    <ServerOperations
      server={entryOf(server)}
      registry={registry}
      health={
        options.running === false
          ? undefined
          : {
              serverId: "srv-bravo",
              ok: true,
              toolCount: 31,
              error: null,
              authRequired: false,
            }
      }
      host={host}
      onRegistryChange={vi.fn()}
    />,
  );
}

const header = () => screen.findByRole("button", { name: /^Advanced/ });
const rowOf = async () => (await header()).closest("[data-operation]") as HTMLElement;
const open = async () => {
  const button = await header();
  if (button.getAttribute("aria-expanded") !== "true") await userEvent.click(button);
  return within(await rowOf());
};
const edits = () => ranArgv().filter((argv) => argv[1] === "edit");
const modes = () => ranArgv().filter((argv) => argv[1] === "mode");
const lastToast = () => vi.mocked(toast.success).mock.calls.at(-1)!;
const undo = () => {
  const action = lastToast()[1]?.action as unknown as { onClick: () => void };
  return act(async () => action.onClick());
};
const limitField = (row: ReturnType<typeof within>) => row.getByLabelText("Limit");

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});
afterEach(() => vi.useRealTimers());

describe("servers.advanced-mode", () => {
  it("test00_the_advanced_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["servers.advanced-mode"]).toMatchObject({
      status: "built",
      test: "src/plus/servers/AdvancedRow.test.tsx",
    });
  });

  it("test01_the_collapsed_row_says_defaults_and_does_not_warn", async () => {
    setup();
    const row = within(await rowOf());
    expect(
      row.getByText(
        "Defaults · all tools listed · no interop switches · calls up to 1 h",
      ),
    ).toBeInTheDocument();
    expect(row.getByText("Defaults")).toBeInTheDocument();
    expect(await header()).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(/need you/)).not.toBeInTheDocument();
  });

  it("test02_the_open_row_has_four_settings_with_a_plain_label_and_the_registry_key", async () => {
    setup();
    const row = await open();
    for (const [label, key] of [
      ["Tool listing", "mode"],
      ["Let the server ask me questions", "declareClientCapabilities"],
      ["Pass the server's instructions to the model", "forwardInstructions"],
      ["Longest a call may run", "maxRequestTimeoutMs"],
    ]) {
      const heading = row.getByRole("heading", { name: `${label} ${key}` });
      expect(within(heading).getByText(key).tagName).toBe("CODE");
    }
    expect(row.getByText(/31 tools in the list/)).toBeInTheDocument();
    expect(row.getAllByText("When to change it")).toHaveLength(3);
  });

  it("test03_a_switch_applies_at_once_and_its_toast_undoes_it", async () => {
    setup();
    const row = await open();
    await userEvent.click(
      row.getByRole("switch", { name: /Pass the server's instructions/ }),
    );
    await waitFor(() =>
      expect(edits()).toEqual([
        ["server", "edit", "srv-bravo", "--forward-instructions", "on"],
      ]),
    );
    const [message, options] = lastToast();
    expect(message).toBe(
      "Pass the server's instructions to the model: on for bravo-build.",
    );
    expect(options).toMatchObject({ action: { label: "Undo" } });
    expect(
      await within(await rowOf()).findByText(
        "All tools listed · 1 switch on · calls up to 1 h",
      ),
    ).toBeInTheDocument();
    expect(within(await rowOf()).getByText("1 changed")).toBeInTheDocument();

    await undo();
    await waitFor(() =>
      expect(edits().at(-1)).toEqual([
        "server",
        "edit",
        "srv-bravo",
        "--forward-instructions",
        "off",
      ]),
    );
    expect(await within(await rowOf()).findByText("Defaults")).toBeInTheDocument();
  });

  it("test04_search_only_applies_at_once_and_says_how_many_tools_leave_the_list", async () => {
    setup();
    const row = await open();
    await userEvent.click(row.getByRole("radio", { name: "Search only" }));
    await waitFor(() =>
      expect(modes()).toEqual([["server", "mode", "srv-bravo", "search"]]),
    );
    expect(lastToast()[0]).toBe(
      "Tool listing for bravo-build: Search only. 31 tools leave the list.",
    );
    expect(
      await row.findByText(/Its 31 tools are left out of the list/),
    ).toBeInTheDocument();
    expect(row.getByRole("radio", { name: "Search only" })).toBeChecked();
    expect(edits()).toEqual([]);

    await undo();
    await waitFor(() =>
      expect(modes().at(-1)).toEqual(["server", "mode", "srv-bravo", "auto"]),
    );
    await waitFor(() => expect(row.getByRole("radio", { name: "Auto" })).toBeChecked());
  });

  it("test05_the_call_limit_is_explained_while_typing_and_saves_only_a_valid_new_value", async () => {
    setup();
    const row = await open();
    const save = row.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    expect(row.getByText("Empty means the default, 1 h.")).toBeInTheDocument();

    const field = limitField(row);
    for (const [text, message] of [
      ["abc", "Enter a number of minutes, for example 90."],
      ["0.5", "At least 1 minute. A shorter wait belongs in the request timeout (Edit)."],
      ["1441", "At most 1,440 minutes (24 hours)."],
    ]) {
      await userEvent.clear(field);
      await userEvent.type(field, text);
      expect(row.getByRole("alert")).toHaveTextContent(message);
      expect(field).toBeInvalid();
      expect(save).toBeDisabled();
    }

    await userEvent.clear(field);
    await userEvent.type(field, "90");
    expect(row.queryByRole("alert")).not.toBeInTheDocument();
    expect(row.getByText(/= 1 h 30 min/)).toBeInTheDocument();
    expect(row.getByTestId("bridge-line")).toHaveTextContent(
      "After saving: 1 h 31 min, set by bravo-build (this server). It was 1 h 1 min.",
    );
    expect(save).toBeEnabled();
    await userEvent.click(save);
    await waitFor(() =>
      expect(edits()).toEqual([
        ["server", "edit", "srv-bravo", "--max-request-timeout-ms", "5400000"],
      ]),
    );
    expect(await row.findByText("Saved. Applies to the next call.")).toBeInTheDocument();
    expect(toast.success).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(row.getByText(/calls up to 1 h 30 min/)).toBeInTheDocument(),
    );
    expect(row.getByText("1 changed")).toBeInTheDocument();
    expect(row.getByTestId("bridge-line")).toHaveTextContent(
      "Right now: 1 h 31 min, set by bravo-build (this server).",
    );
  });

  it("test06_use_default_removes_the_limit_and_is_off_while_there_is_none", async () => {
    setup({ server: { maxRequestTimeoutMs: 5_400_000 } });
    const row = await open();
    expect(limitField(row)).toHaveValue("90");
    await userEvent.click(row.getByRole("button", { name: "Use default (60)" }));
    await waitFor(() =>
      expect(edits()).toEqual([
        ["server", "edit", "srv-bravo", "--max-request-timeout-ms", "default"],
      ]),
    );
    await waitFor(() => expect(limitField(row)).toHaveValue(""));
    expect(row.getByRole("button", { name: "Use default (60)" })).toBeDisabled();
  });

  it("test07_a_limit_shorter_than_the_request_timeout_says_it_has_no_effect_and_can_still_be_saved", async () => {
    setup({ server: { requestTimeoutMs: 600_000 } });
    const row = await open();
    expect(
      row.getByText(/10 min without a sign of life, set in Edit/),
    ).toBeInTheDocument();
    await userEvent.type(limitField(row), "5");
    expect(row.getByText(/so 5 min has no effect/)).toBeInTheDocument();
    expect(row.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("test08_another_server_that_waits_longer_keeps_setting_the_bridge", async () => {
    setup({
      others: [
        stdioServer("srv-delta", "delta-sync", { maxRequestTimeoutMs: 7_200_000 }),
      ],
    });
    const row = await open();
    expect(row.getByTestId("bridge-line")).toHaveTextContent(
      "Right now: 2 h 1 min, set by delta-sync.",
    );
    await userEvent.type(limitField(row), "90");
    expect(row.getByTestId("bridge-line")).toHaveTextContent(
      "Stays 2 h 1 min, set by delta-sync.",
    );
  });

  it("test09_a_limit_under_a_minute_from_the_command_line_is_read_only_text", async () => {
    setup({ server: { maxRequestTimeoutMs: 30_000 } });
    const row = await open();
    expect(row.queryByLabelText("Limit")).not.toBeInTheDocument();
    expect(row.getByText("30 s")).toBeInTheDocument();
    expect(row.getByText(/toolportctl server edit bravo-build/)).toBeInTheDocument();
    expect(row.getByText(/calls up to 30 s/, { selector: "span" })).toBeInTheDocument();
  });

  it("test10_a_remote_server_gets_a_note_instead_of_the_call_limit", async () => {
    setup({
      server: { transport: "http", command: null, url: "https://example.invalid/mcp" },
    });
    const row = await open();
    expect(row.getByText("Local servers only")).toBeInTheDocument();
    expect(row.getByText(/Not used for remote servers/)).toBeInTheDocument();
    expect(row.queryByLabelText("Limit")).not.toBeInTheDocument();
    expect(
      row.getByText("Defaults · all tools listed · no interop switches"),
    ).toBeInTheDocument();
  });
});

describe("a change that needs a restart", () => {
  const questions = (row: ReturnType<typeof within>) =>
    row.getByRole("switch", { name: /Let the server ask me questions/ });

  it("test11_says_the_running_server_was_not_told_and_restarts_only_when_asked", async () => {
    setup();
    const row = await open();
    expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument();
    await userEvent.click(questions(row));
    expect(await row.findByText(/Not told yet\./)).toBeInTheDocument();
    expect(
      row.getByText(/Toolport restarts it once then, and not while a call is running/),
    ).toBeInTheDocument();
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toEqual([]);

    await userEvent.click(row.getByRole("button", { name: "Restart now" }));
    await waitFor(() =>
      expect(ranArgv().filter((argv) => argv[1] === "restart")).toEqual([
        ["server", "restart", "srv-bravo", "--if-idle"],
      ]),
    );
    await waitFor(() => expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument());
  });

  it("test12_a_server_a_gateway_still_has_to_tell_says_so_after_a_reload_without_any_toggle", async () => {
    setup({ server: { declareClientCapabilities: true, pendingRedeclare: true } });
    const row = await open();
    expect(await row.findByText(/Not told yet\./)).toBeInTheDocument();
    expect(edits()).toEqual([]);

    await userEvent.click(row.getByRole("button", { name: "Restart now" }));
    await waitFor(() => expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument());
  });

  it("test13_a_server_that_is_up_to_date_or_not_running_has_no_hint", async () => {
    setup({ server: { declareClientCapabilities: true } });
    const row = await open();
    await waitFor(() =>
      expect(row.getByRole("switch", { name: /questions/ })).toBeChecked(),
    );
    expect(row.queryByText(/Not told yet|Still told/)).not.toBeInTheDocument();
  });

  it("test14_a_pending_server_that_is_not_running_has_nothing_to_restart", async () => {
    setup({
      running: false,
      server: { declareClientCapabilities: true, pendingRedeclare: true },
    });
    const row = await open();
    await waitFor(() =>
      expect(row.getByRole("switch", { name: /questions/ })).toBeChecked(),
    );
    expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument();
  });

  it("test15_flipping_it_back_before_a_restart_takes_the_hint_away", async () => {
    setup();
    const row = await open();
    await userEvent.click(questions(row));
    await row.findByText(/Not told yet\./);
    await undo();
    await waitFor(() => expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument());
  });

  it("test16_turning_it_off_says_the_running_server_is_still_told", async () => {
    setup({ server: { declareClientCapabilities: true } });
    const row = await open();
    await userEvent.click(questions(row));
    expect(await row.findByText(/Still told\./)).toBeInTheDocument();
    expect(row.getByText(/stops being told at its next start/)).toBeInTheDocument();
  });

  it("test17_a_running_call_sends_the_question_to_the_runtime_row", async () => {
    setup({ restart: { kind: "busy" } });
    const row = await open();
    await userEvent.click(questions(row));
    await userEvent.click(await row.findByRole("button", { name: "Restart now" }));
    expect(
      await row.findByText(/A call is running\. The Runtime row asks/),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("group", { name: "Calls in flight" }),
    ).toBeInTheDocument();
  });

  it("test18_a_server_that_is_not_running_has_nothing_to_restart", async () => {
    setup({ running: false });
    const row = await open();
    expect(
      row.getByText(/is not connected yet\. Your settings are saved/),
    ).toBeInTheDocument();
    await userEvent.click(questions(row));
    await waitFor(() => expect(edits()).toHaveLength(1));
    expect(row.queryByText(/Not told yet/)).not.toBeInTheDocument();
    expect(row.queryByRole("button", { name: "Restart now" })).not.toBeInTheDocument();
  });

  it("test19_a_server_that_is_off_keeps_the_settings_for_its_next_start", async () => {
    setup({ server: { enabled: false } });
    const row = await open();
    expect(
      row.getByText(/bravo-build is off\. Your settings are saved/),
    ).toBeInTheDocument();
  });
});
