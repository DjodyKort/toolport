import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { fixtureServers, type RestartScript } from "../fixtures/serverWorld";
import { GatewayAnswers } from "./RuntimeRow";
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
  toast: Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    message: vi.fn(),
  }),
}));

const servers = fixtureServers();
const sets = [
  { id: "local", name: "Local fixture", serverIds: servers.map((s) => s.id) },
];
const registry = registryOf(servers, sets);
const alpha = entryOf(servers[0]);
const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

function start(script?: RestartScript, extra: { enabled?: boolean } = {}) {
  const list = fixtureServers();
  if (extra.enabled === false) list[0].enabled = false;
  resetPlusFixture({
    servers: { servers: list, restart: script ? { "fixture-0": script } : {} },
  });
  const entry = extra.enabled === false ? entryOf(list[0]) : alpha;
  const reg = extra.enabled === false ? registryOf(list, sets) : registry;
  render(
    <ServerOperations
      server={entry}
      registry={reg}
      health={{
        serverId: "fixture-0",
        ok: true,
        toolCount: 31,
        error: null,
        authRequired: false,
      }}
      host={host}
      onRegistryChange={vi.fn()}
    />,
  );
}

const runtime = () =>
  screen.getByRole("button", { name: /^Runtime/ }).closest("[data-operation]")!;
const restartButton = () =>
  within(runtime() as HTMLElement).getByRole("button", { name: "Restart" });

beforeEach(() => {
  getRegistryMock.mockReset().mockResolvedValue(registry);
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});
afterEach(() => vi.useRealTimers());

describe("servers.restart", () => {
  it("test00_the_restart_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["servers.restart"]).toMatchObject({
      status: "built",
      test: "src/plus/servers/RuntimeRow.test.tsx",
    });
  });

  it("test01_an_idle_server_restarts_with_one_click_and_lists_every_gateway", async () => {
    start();
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    expect(await screen.findByText(/GitHub restarted in 3 gateways/)).toBeInTheDocument();
    expect(
      screen.queryByRole("group", { name: "Calls in flight" }),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /^Runtime/ }));
    const answers = screen.getByRole("list", { name: "Gateway answers" });
    expect(within(answers).getAllByRole("listitem")).toHaveLength(3);
    expect(toast.success).toHaveBeenCalledWith("GitHub restarted in 3 gateways");
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toEqual([
      ["server", "restart", "fixture-0", "--if-idle"],
    ]);
  });

  it("test02_while_the_gateways_answer_the_row_says_so_and_the_button_waits", async () => {
    start({ kind: "ok", delayMs: 300 });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    expect(await screen.findByText(/Asking every running gateway/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restarting" })).toBeDisabled();
    expect(await screen.findByText(/GitHub restarted in 3 gateways/)).toBeInTheDocument();
  });

  it("test03_a_call_in_flight_asks_once_and_restart_now_ends_it", async () => {
    start({ kind: "busy" });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    const card = await screen.findByRole("group", { name: "Calls in flight" });
    expect(within(card).getByText("1 call is still running")).toBeInTheDocument();
    expect(within(card).getByText(/Gateway 4121/)).toBeInTheDocument();
    expect(within(card).queryByText(/Gateway 4188/)).not.toBeInTheDocument();
    await userEvent.click(within(card).getByRole("button", { name: "Restart now" }));
    expect(await screen.findByText(/GitHub restarted in 3 gateways/)).toBeInTheDocument();
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toEqual([
      ["server", "restart", "fixture-0", "--if-idle"],
      ["server", "restart", "fixture-0"],
    ]);
  });

  it("test04_cancel_leaves_a_busy_server_alone", async () => {
    start({ kind: "busy" });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    const card = await screen.findByRole("group", { name: "Calls in flight" });
    await userEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("group", { name: "Calls in flight" }),
    ).not.toBeInTheDocument();
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toHaveLength(1);
    expect(restartButton()).toBeEnabled();
  });

  it("test05_wait_then_restart_asks_again_until_the_call_is_over", async () => {
    start({ kind: "busy", busyProbes: 2 });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    const card = await screen.findByRole("group", { name: "Calls in flight" });
    vi.useFakeTimers({ shouldAdvanceTime: true });
    fireEvent.click(within(card).getByRole("button", { name: "Wait, then restart" }));
    expect(await screen.findByText(/Waiting for the running call/)).toBeInTheDocument();
    await act(() => vi.advanceTimersByTimeAsync(2_100));
    await act(() => vi.advanceTimersByTimeAsync(2_100));
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("GitHub restarted in 3 gateways"),
    );
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toHaveLength(3);
  });

  it("test06_one_failed_gateway_is_named_and_the_others_keep_their_new_process", async () => {
    start({ kind: "partial" });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    expect(
      await screen.findByText(/GitHub: 1 of 3 gateways could not restart it/),
    ).toBeInTheDocument();
    expect(screen.getByText("1 failed")).toBeInTheDocument();
    const answers = screen.getByRole("list", { name: "Gateway answers" });
    expect(within(answers).getByText(/uv: command not found/)).toBeInTheDocument();
    expect(within(answers).getAllByText(/restarted/)).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(screen.getByText(/restarts once more/)).toBeInTheDocument();
    expect(toast.error).toHaveBeenCalled();
  });

  it("test07_no_running_gateway_is_information_and_not_an_error", async () => {
    start({ kind: "none" });
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    expect(
      await screen.findByText(/No gateway holds GitHub right now/),
    ).toBeInTheDocument();
    expect(screen.getByText("Idle")).toBeInTheDocument();
    expect(screen.getByText(/has already happened/)).toBeInTheDocument();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("test08_a_server_that_is_off_has_no_restart", async () => {
    start(undefined, { enabled: false });
    expect(
      await screen.findByText(/Switched off · nothing is running/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restart" })).not.toBeInTheDocument();
  });

  it("test09_a_bridge_that_cannot_start_says_so_in_the_row", async () => {
    resetPlusFixture({ down: true });
    render(
      <ServerOperations
        server={alpha}
        registry={registry}
        host={host}
        onRegistryChange={vi.fn()}
      />,
    );
    await userEvent.click(await screen.findByRole("button", { name: "Restart" }));
    expect(await screen.findByText(/Could not ask the gateways/)).toBeInTheDocument();
    expect(restartButton()).toBeEnabled();
  });

  it("test10_gateway_rows_show_the_project_folder_when_the_answer_carries_one", () => {
    render(
      <GatewayAnswers
        gateways={[
          { pid: 11, restarted: true, detail: "restarted", root: "~/work/project-a" },
          { pid: 12, restarted: true, detail: "restarted" },
        ]}
      />,
    );
    expect(screen.getByText("~/work/project-a")).toBeInTheDocument();
    expect(screen.getByText("Gateway 12")).toBeInTheDocument();
    expect(screen.getByText(/\(gateway 11\)/)).toBeInTheDocument();
  });
});
