import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import type { Registry } from "@/lib/types";
import type { WorldOptions } from "../fixtures/ctlWorld";
import { plusFixtureWorld, resetPlusFixture } from "../fixtures/ipc";
import { stdioServer } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { PlusAccessCompare } from "../servers/slots";
import { invokeLog, ranArgv, registryOf } from "../servers/testkit";
import AccessWrite from "./AccessWrite";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const servers = [
  stdioServer("srv-alpha", "alpha-api"),
  stdioServer("srv-beta", "beta-crm"),
  stdioServer("srv-gamma", "gamma-docs", { enabled: false }),
];
const sets = [
  { id: "all", name: "All servers", serverIds: ["srv-alpha", "srv-beta", "srv-gamma"] },
  { id: "focus", name: "Focus", serverIds: ["srv-alpha"] },
  { id: "spare", name: "Spare", serverIds: ["srv-beta"] },
];
const reach: WorldOptions["reach"] = {
  clients: [
    { id: "claude-code", name: "Claude Code", set: "focus" },
    { id: "cursor", name: "Cursor" },
  ],
  folders: [{ path: "/work/proj", set: "focus" }],
};

let registry: Registry;
const changes = vi.fn();

/** Keeps the registry the screen was last told about, as upstream's page does. */
function Harness() {
  const [current, setCurrent] = useState(registry);
  return (
    <AccessWrite
      registry={current}
      onRegistryChange={(next) => {
        changes(next);
        setCurrent(next);
      }}
    />
  );
}

function setup(options: WorldOptions = {}, defaultAccess: string | null = "all") {
  resetPlusFixture({
    servers: { servers, sets, defaultAccess },
    reach,
    ...options,
  });
  registry = registryOf(servers, sets, { defaultAccess });
  return render(<Harness />);
}

const wrote = (...words: string[]) =>
  ranArgv().some(
    (argv) => words.every((word) => argv.includes(word)) && !argv.includes("--dry-run"),
  );
const planned = (...words: string[]) =>
  ranArgv().some(
    (argv) => words.every((word) => argv.includes(word)) && argv.includes("--dry-run"),
  );
const toolRun = () =>
  invokeLog
    .filter((entry) => entry.command === "plus_ctl")
    .filter(
      (entry) => (entry.args as { argv: string[] }).argv[2] === "client_discovery_set",
    )
    .map((entry) => JSON.parse((entry.args as { stdinSecret: string }).stdinSecret));

const row = (name: string) =>
  screen.getByText(name, { selector: "span.text-sm" }).closest("div.flex-col")!
    .parentElement as HTMLElement;

async function confirm(user: ReturnType<typeof userEvent.setup>, label: string) {
  await user.click(await screen.findByRole("button", { name: label }));
}

/** Clicks the Undo of the last toast of `kind` and waits for it to be done. */
async function undo() {
  const [, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
  (options?.action as unknown as { onClick: () => void }).onClick();
}

beforeEach(() => {
  invokeLog.length = 0;
  changes.mockReset();
  vi.clearAllMocks();
});

describe("Change access sets", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["access.write"]).toMatchObject({
      status: "built",
      test: "src/plus/access/AccessWrite.test.tsx",
    });
    expect(guiParity.routes["access-write"]).toMatchObject({
      status: "built",
      page: "settings:access",
    });
  });

  it("test01_access_write_lists_the_sets_the_clients_and_the_folder_routes", async () => {
    setup();
    expect(await screen.findByText("Default access")).toBeInTheDocument();
    expect(
      screen.getByText("Clients without a set of their own use All servers."),
    ).toBeVisible();
    const focus = row("Focus");
    expect(
      within(focus).getByText("1 server, 1 on · used by 1 client and 1 folder route"),
    ).toBeVisible();
    expect(within(row("All servers")).getByText("default")).toBeVisible();
    expect(screen.getByText("uses Focus · tool listing: global mode")).toBeVisible();
    expect(
      screen.getByText("follows the default access · tool listing: global mode"),
    ).toBeVisible();
    expect(screen.getByText("/work/proj")).toBeVisible();
    expect(
      screen.getByText(/A folder route outranks a client's own access set/),
    ).toBeVisible();
  });

  it("test02_access_write_creates_a_set_after_its_plan_and_undo_deletes_it_again", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "New access set…" }));
    const form = await screen.findByRole("dialog");
    const next = within(form).getByRole("button", { name: "Preview the plan" });
    expect(next).toBeDisabled();
    await user.type(within(form).getByLabelText("Name"), "focus");
    expect(within(form).getByRole("alert")).toHaveTextContent("exists already");
    expect(next).toBeDisabled();
    await user.clear(within(form).getByLabelText("Name"));
    await user.type(within(form).getByLabelText("Name"), "Deep work");
    await user.click(within(form).getByRole("checkbox", { name: "alpha-api" }));
    await user.click(next);
    expect(await screen.findByText("Add access set Deep work")).toBeVisible();
    const plan = screen.getByRole("list", { name: "Plan" });
    expect(within(plan).getByText("list alpha-api in Deep work")).toBeVisible();
    expect(planned("access-set", "add", "Deep work", "srv-alpha")).toBe(true);
    expect(wrote("access-set", "add")).toBe(false);
    await confirm(user, "Add access set");
    await waitFor(() =>
      expect(wrote("access-set", "add", "Deep work", "srv-alpha")).toBe(true),
    );
    expect(await screen.findByText("Deep work")).toBeVisible();
    expect(changes.mock.calls.at(-1)![0].profiles.at(-1)).toEqual({
      id: "deep-work",
      name: "Deep work",
      enabledServerIds: ["srv-alpha"],
    });
    expect(vi.mocked(toast.success).mock.calls.at(-1)![0]).toBe("Added Deep work");
    await undo();
    await waitFor(() => expect(screen.queryByText("Deep work")).not.toBeInTheDocument());
    expect(wrote("access-set", "rm", "Deep work")).toBe(true);
  });

  it("test03_access_write_deletes_an_unused_set_and_undo_brings_it_back_with_its_servers", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    expect(await screen.findByText("Delete access set Spare")).toBeVisible();
    expect(wrote("access-set", "rm")).toBe(false);
    await confirm(user, "Delete access set");
    await waitFor(() => expect(screen.queryByText("Spare")).not.toBeInTheDocument());
    expect(vi.mocked(toast.success).mock.calls.at(-1)![0]).toBe("Deleted Spare");
    await undo();
    expect(await screen.findByText("Spare")).toBeVisible();
    expect(wrote("access-set", "add", "Spare", "--restore")).toBe(true);
  });

  it("test03b_access_write_undo_of_a_delete_restores_the_tool_lists_and_the_instructions_too", async () => {
    const user = userEvent.setup();
    setup({
      access: {
        narrowed: [{ set: "spare", server: "srv-beta", tools: ["read", "list"] }],
        instructions: { spare: "Be brief" },
      },
    });
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    await confirm(user, "Delete access set");
    await waitFor(() => expect(screen.queryByText("Spare")).not.toBeInTheDocument());
    const world = plusFixtureWorld();
    expect(world.access.instructions.has("spare")).toBe(false);
    await undo();
    expect(await screen.findByText("Spare")).toBeVisible();
    expect(world.access.instructions.get("spare")).toBe("Be brief");
    const tools = world.run(["access-set", "tools", "spare"]).data as {
      accessSets: Array<{ narrowed: Array<{ tools: string[] }> }>;
    };
    expect(tools.accessSets[0].narrowed[0].tools).toEqual(["read", "list"]);
  });

  it("test03c_access_write_undo_of_a_delete_that_was_already_restored_says_why_it_failed", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    await confirm(user, "Delete access set");
    await waitFor(() => expect(screen.queryByText("Spare")).not.toBeInTheDocument());
    await undo();
    expect(await screen.findByText("Spare")).toBeVisible();
    await undo();
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not undo", {
        description: expect.stringContaining("no removed access set"),
        action: expect.anything(),
      }),
    );
  });

  it("test04_access_write_asks_what_happens_to_what_uses_a_set_and_undo_restores_the_bindings", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Focus" }));
    const choice = await screen.findByRole("dialog");
    expect(
      within(choice).getByText(
        /still used by client claude-code and folder route \/work\/proj/,
      ),
    ).toBeVisible();
    const next = within(choice).getByRole("button", { name: "Preview the plan" });
    expect(next).toBeDisabled();
    await user.click(
      within(choice).getByRole("radio", { name: /Take their binding away/ }),
    );
    await user.click(next);
    expect(
      await screen.findByText("remove the binding of client claude-code"),
    ).toBeVisible();
    expect(screen.getByText("remove the binding of folder /work/proj")).toBeVisible();
    await confirm(user, "Delete access set");
    await waitFor(() => expect(wrote("access-set", "rm", "--detach")).toBe(true));
    await waitFor(() => expect(screen.queryByText("Focus")).not.toBeInTheDocument());
    expect(screen.getByText("No folder is routed")).toBeVisible();
    expect(
      changes.mock.calls.at(-1)![0].profiles.map((p: { id: string }) => p.id),
    ).toEqual(["all", "spare"]);
    await undo();
    expect(await screen.findByText("/work/proj")).toBeVisible();
    expect(
      await screen.findByText("uses Focus · tool listing: global mode"),
    ).toBeVisible();
    expect(wrote("access-set", "add", "Focus", "--restore")).toBe(true);
    expect(wrote("client", "scope")).toBe(false);
  });

  it("test05_access_write_leaves_the_default_alone_and_deleting_it_is_not_offered", async () => {
    setup();
    await screen.findByText("Default access");
    expect(screen.getByRole("button", { name: "Delete All servers" })).toBeDisabled();
  });

  it("test06_access_write_changes_the_default_access_and_undo_returns_it", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Make Focus the default access" }),
    );
    expect(
      await screen.findByText("Make Focus the default access", { selector: "p" }),
    ).toBeVisible();
    await confirm(user, "Make default");
    await waitFor(() => expect(within(row("Focus")).getByText("default")).toBeVisible());
    expect(changes.mock.calls.at(-1)![0].defaultAccessProfileId).toBe("focus");
    await undo();
    await waitFor(() =>
      expect(within(row("All servers")).getByText("default")).toBeVisible(),
    );
    await user.click(screen.getByRole("button", { name: "Use every enabled server…" }));
    await confirm(user, "Use every enabled server");
    await waitFor(() => expect(wrote("access-set", "default", "--none")).toBe(true));
    await waitFor(() =>
      expect(
        screen.getByText(
          "Clients without a set of their own reach every enabled server.",
        ),
      ).toBeVisible(),
    );
    expect(
      screen.queryByRole("button", { name: "Use every enabled server…" }),
    ).toBeNull();
  });

  it("test07_access_write_narrows_the_tools_of_a_server_and_undo_exposes_them_all_again", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Choose the tools of All servers" }),
    );
    const form = await screen.findByRole("dialog");
    await user.selectOptions(within(form).getByLabelText("Server"), "beta-crm");
    await user.click(within(form).getByRole("radio", { name: "Only the tools below" }));
    const next = within(form).getByRole("button", { name: "Preview the plan" });
    expect(next).toBeDisabled();
    await user.type(
      within(form).getByLabelText("Tool names"),
      "read_file, list_dir{enter}read_file",
    );
    await user.click(next);
    expect(
      await screen.findByText("Limit beta-crm in All servers to 2 tool(s)"),
    ).toBeVisible();
    expect(screen.getByText(/tool names are not checked/)).toBeVisible();
    expect(
      ranArgv().some(
        (a) => a.includes("tools") && a.includes("--dry-run") && a.includes("read_file"),
      ),
    ).toBe(true);
    await confirm(user, "Change the tools");
    await waitFor(() =>
      expect(wrote("access-set", "tools", "--tool", "list_dir")).toBe(true),
    );
    expect(changes.mock.calls.at(-1)![0].profiles[0].toolScope).toEqual({
      "srv-beta": ["read_file", "list_dir"],
    });
    await undo();
    await waitFor(() => expect(wrote("access-set", "tools", "--all")).toBe(true));
  });

  it("test08_access_write_binds_a_client_to_a_set_and_unbinds_it_again", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Change the access set of Cursor" }),
    );
    const form = await screen.findByRole("dialog");
    await user.selectOptions(within(form).getByLabelText("Access set"), "Spare");
    await user.click(within(form).getByRole("button", { name: "Preview the plan" }));
    expect(await screen.findByText("Bind cursor to the access set Spare")).toBeVisible();
    await confirm(user, "Bind to Spare");
    expect(
      await screen.findByText("uses Spare · tool listing: global mode"),
    ).toBeVisible();
    expect(changes.mock.calls.at(-1)![0].clientScopes.cursor).toBe("spare");
    await undo();
    await waitFor(() =>
      expect(
        screen.queryByText("uses Spare · tool listing: global mode"),
      ).not.toBeInTheDocument(),
    );
    expect(wrote("client", "scope", "cursor", "--none")).toBe(true);
  });

  it("test09_access_write_says_nothing_changes_when_the_client_already_uses_the_set", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Change the access set of Claude Code" }),
    );
    await user.click(await screen.findByRole("button", { name: "Preview the plan" }));
    expect(await screen.findByText("Nothing to change.")).toBeVisible();
    expect(screen.getByRole("button", { name: /Bind to Focus/ })).toBeDisabled();
    expect(wrote("client", "scope")).toBe(false);
  });

  it("test10_access_write_pins_the_tool_listing_of_a_client_through_the_tool_with_confirm", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Change the tool listing of Cursor" }),
    );
    const form = await screen.findByRole("dialog");
    await user.selectOptions(within(form).getByLabelText("Tool listing"), "grouped");
    await user.click(within(form).getByRole("button", { name: "Preview the plan" }));
    expect(
      await screen.findByText("Set the discovery mode of cursor to grouped"),
    ).toBeVisible();
    expect(toolRun()).toEqual([{ client: "cursor", mode: "grouped" }]);
    await confirm(user, "Set the tool listing");
    await waitFor(() => expect(toolRun()).toHaveLength(2));
    expect(toolRun()[1]).toEqual({
      client: "cursor",
      mode: "grouped",
      dry_run: false,
      confirm: true,
    });
    expect(changes.mock.calls.at(-1)![0].clientDiscovery).toEqual({ cursor: "grouped" });
    expect(
      await screen.findByText("follows the default access · tool listing: Grouped"),
    ).toBeVisible();
    await undo();
    await waitFor(() => expect(toolRun()).toHaveLength(3));
    expect(toolRun()[2]).toEqual({
      client: "cursor",
      mode: "inherit",
      dry_run: false,
      confirm: true,
    });
  });

  it("test11_access_write_routes_a_folder_and_refuses_a_relative_path", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Route a folder…" }));
    const form = await screen.findByRole("dialog");
    const next = within(form).getByRole("button", { name: "Preview the plan" });
    await user.type(within(form).getByLabelText("Folder"), "work/relative");
    expect(within(form).getByRole("alert")).toHaveTextContent("full folder path");
    expect(next).toBeDisabled();
    await user.clear(within(form).getByLabelText("Folder"));
    await user.type(within(form).getByLabelText("Folder"), "/work/other");
    await user.selectOptions(within(form).getByLabelText("Access set"), "Spare");
    await user.click(next);
    expect(
      await screen.findByText("Route /work/other to the access set Spare"),
    ).toBeVisible();
    expect(screen.getByText(/Clients with their own set: claude-code/)).toBeVisible();
    await confirm(user, "Route the folder");
    expect(await screen.findByText("/work/other")).toBeVisible();
    expect(changes.mock.calls.at(-1)![0].folderProfiles).toContainEqual({
      path: "/work/other",
      profile: "spare",
    });
    await undo();
    await waitFor(() =>
      expect(screen.queryByText("/work/other")).not.toBeInTheDocument(),
    );
    expect(wrote("folder", "route", "rm", "/work/other")).toBe(true);
  });

  it("test12_access_write_moves_a_route_to_another_set_and_removes_it_with_undo", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(
      screen.getByRole("button", { name: "Change the access set of /work/proj" }),
    );
    const form = await screen.findByRole("dialog");
    await user.selectOptions(within(form).getByLabelText("Access set"), "Spare");
    await user.click(within(form).getByRole("button", { name: "Preview the plan" }));
    await confirm(user, "Route the folder");
    expect(await screen.findByText("Spare · 1 server")).toBeVisible();
    await undo();
    await waitFor(() => expect(screen.getByText("Focus · 1 server")).toBeVisible());
    await user.click(
      screen.getByRole("button", { name: "Remove the route of /work/proj" }),
    );
    expect(
      await screen.findByText("Remove the folder route for /work/proj"),
    ).toBeVisible();
    await confirm(user, "Remove the route");
    expect(await screen.findByText("No folder is routed")).toBeVisible();
    await undo();
    expect(await screen.findByText("/work/proj")).toBeVisible();
    expect(wrote("folder", "route", "add", "/work/proj", "Focus")).toBe(true);
  });

  it("test13_access_write_keeps_the_plan_open_with_the_reason_when_the_write_fails", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    await screen.findByText("Delete access set Spare");
    resetPlusFixture({
      servers: { servers, sets: sets.filter((s) => s.id !== "spare") },
      reach,
    });
    await confirm(user, "Delete access set");
    expect(
      await screen.findByText(/Nothing was changed: no access set called/),
    ).toBeVisible();
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("test14_access_write_shows_the_refusal_of_the_plan_and_offers_no_confirm", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    await screen.findByText("Delete access set Spare");
    resetPlusFixture({ servers: { servers, sets: [sets[0]] }, reach });
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("no access set called");
    expect(screen.getByRole("button", { name: "Delete access set" })).toBeDisabled();
  });

  it("test15_access_write_escape_closes_a_plan_without_writing", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "Delete Spare" }));
    await screen.findByText("Delete access set Spare");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(wrote("access-set", "rm")).toBe(false);
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("test16_access_write_says_so_when_there_is_nothing_yet", async () => {
    setup({ reach: {} }, null);
    expect(await screen.findByText("No folder is routed")).toBeVisible();
    expect(
      screen.getByText("Clients without a set of their own reach every enabled server."),
    ).toBeVisible();
    expect(screen.getByText("No client holds Toolport's gateway entry")).toBeVisible();
  });

  it("test17_access_write_offers_a_retry_when_the_bridge_is_down", async () => {
    resetPlusFixture({ down: true });
    registry = registryOf(servers, sets);
    render(<Harness />);
    expect(
      await screen.findByText("The access sets could not be read for changing"),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  });

  it("test18_access_write_cancels_a_form_without_a_plan", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("Default access");
    await user.click(screen.getByRole("button", { name: "New access set…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(ranArgv().filter((argv) => argv.includes("--dry-run"))).toEqual([]);
  });

  it("test19_access_write_sits_under_the_compare_table_on_the_access_page", async () => {
    resetPlusFixture({ servers: { servers, sets, defaultAccess: "all" }, reach });
    registry = registryOf(servers, sets);
    render(<PlusAccessCompare registry={registry} onRegistryChange={vi.fn()} />);
    expect(
      await screen.findByRole("region", { name: "Compare access sets" }),
    ).toBeVisible();
    expect(
      await screen.findByRole("region", { name: "Change access sets" }),
    ).toBeVisible();
  });
});
