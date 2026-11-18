import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import type { WorldOptions } from "../fixtures/ctlWorld";
import { resetPlusFixture } from "../fixtures/ipc";
import { stdioServer } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import AccessCompare from "./AccessCompare";
import { identicalSets } from "./model";
import { getRegistryMock, invokeLog, ranArgv, registryOf } from "./testkit";

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

const COVERS = [
  "servers.access-sets",
  "servers.access-set-member",
  "servers.reach",
] as const;

const servers = [
  stdioServer("srv-alpha", "alpha-api"),
  stdioServer("srv-beta", "beta-crm"),
  stdioServer("srv-gamma", "gamma-docs", { enabled: false }),
];
const sets = [
  { id: "all", name: "All servers", serverIds: ["srv-alpha", "srv-beta", "srv-gamma"] },
  { id: "claude-code", name: "claude-code", serverIds: ["srv-alpha"] },
  { id: "claude-desktop", name: "claude-desktop", serverIds: ["srv-alpha"] },
];
const registry = registryOf(servers, sets);

function setup(
  next = sets,
  reach?: WorldOptions["reach"],
  defaultAccess?: string | null,
) {
  resetPlusFixture({ servers: { servers, sets: next, defaultAccess }, reach });
  getRegistryMock.mockResolvedValue(registry);
  const onRegistryChange = vi.fn();
  render(<AccessCompare registry={registry} onRegistryChange={onRegistryChange} />);
  return { onRegistryChange };
}

const cell = (server: string, set: string) =>
  screen.getByRole("button", { name: `${server} in ${set}` });

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Compare access sets", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/servers/AccessCompare.test.tsx",
      });
    }
  });

  it("test01_servers_access_sets_shows_servers_by_set_in_one_table", async () => {
    setup();
    const table = await screen.findByRole("table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((h) => h.textContent),
    ).toEqual([
      "Server",
      "All servers3 serversnone",
      "claude-code1 servernone",
      "claude-desktop1 servernonesame as claude-code",
    ]);
    expect(
      within(table)
        .getAllByRole("rowheader")
        .map((h) => h.textContent),
    ).toEqual(["alpha-api", "beta-crm", "gamma-docsOff"]);
    expect(cell("alpha-api", "claude-code")).toHaveAttribute("aria-pressed", "true");
    expect(cell("beta-crm", "claude-code")).toHaveAttribute("aria-pressed", "false");
    expect(ranArgv()).toContainEqual([
      "mcp",
      "call",
      "servers_list_profiles",
      "--args-stdin",
    ]);
    expect(within(table).getByText("gamma-docs").closest("tr")).toHaveClass("opacity-60");
  });

  it("test02_servers_access_set_member_toggles_a_cell_at_once_and_undo_puts_it_back", async () => {
    const { onRegistryChange } = setup();
    await screen.findByRole("table");
    await userEvent.click(cell("beta-crm", "claude-code"));
    await waitFor(() =>
      expect(cell("beta-crm", "claude-code")).toHaveAttribute("aria-pressed", "true"),
    );
    expect(onRegistryChange).toHaveBeenCalled();
    const [message, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
    expect(message).toBe("beta-crm is in claude-code");
    (options?.action as unknown as { onClick: () => void }).onClick();
    await waitFor(() =>
      expect(cell("beta-crm", "claude-code")).toHaveAttribute("aria-pressed", "false"),
    );
    const tools = invokeLog
      .filter((e) => e.command === "plus_ctl")
      .map((e) => (e.args as { argv: string[] }).argv[2])
      .filter((tool) => tool?.endsWith("profile_tag"));
    expect(tools).toEqual(["servers_add_profile_tag", "servers_remove_profile_tag"]);
  });

  it("test03_servers_access_set_member_keeps_the_memberships_of_a_server_that_is_off", async () => {
    setup();
    await screen.findByRole("table");
    expect(cell("gamma-docs", "All servers")).toHaveAttribute("aria-pressed", "true");
  });

  it("test04_servers_access_set_member_names_the_server_when_the_write_fails", async () => {
    setup();
    await screen.findByRole("table");
    resetPlusFixture({ servers: { servers: servers.slice(1), sets } });
    await userEvent.click(cell("alpha-api", "claude-code"));
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Could not change claude-code", {
        description: "server not found: srv-alpha",
        action: expect.anything(),
      }),
    );
    expect(cell("alpha-api", "claude-code")).toHaveAttribute("aria-pressed", "true");
  });

  it("test05_servers_access_sets_labels_identical_sets_and_merges_nothing", () => {
    expect(identicalSets(sets)).toEqual(new Map([["claude-desktop", "claude-code"]]));
    expect(identicalSets(sets.slice(0, 2))).toEqual(new Map());
  });

  it("test06_servers_access_sets_arrow_keys_move_between_cells", async () => {
    setup();
    await screen.findByRole("table");
    cell("alpha-api", "All servers").focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(cell("alpha-api", "claude-code")).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(cell("beta-crm", "claude-code")).toHaveFocus();
    await userEvent.keyboard("{ArrowLeft}{ArrowUp}");
    expect(cell("alpha-api", "All servers")).toHaveFocus();
  });

  it("test07_servers_access_sets_has_a_chip_list_for_a_narrow_window", async () => {
    setup();
    await screen.findByRole("table");
    const chip = screen.getByRole("button", { name: "beta-crm in claude-code (list)" });
    await userEvent.click(chip);
    await waitFor(() => expect(chip).toHaveAttribute("aria-pressed", "true"));
  });

  it("test08_servers_access_sets_says_so_when_there_are_no_sets", async () => {
    setup([]);
    expect(await screen.findByText("There are no access sets yet.")).toBeInTheDocument();
  });

  it("test09_servers_access_sets_offers_a_retry_when_the_bridge_is_down", async () => {
    resetPlusFixture({ down: true });
    render(<AccessCompare registry={registry} onRegistryChange={vi.fn()} />);
    expect(
      await screen.findByText("The access sets could not be read"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("test10_servers_access_sets_says_who_uses_each_set_under_its_column", async () => {
    setup(
      sets,
      {
        clients: [{ id: "claude-code", name: "Claude Code", set: "claude-code" }],
        folders: [{ path: "/work/proj", set: "claude-desktop" }],
      },
      "all",
    );
    const table = await screen.findByRole("table");
    await waitFor(() =>
      expect(
        within(table)
          .getAllByRole("columnheader")
          .map((h) => h.textContent),
      ).toEqual([
        "Server",
        "All servers3 serversdefault",
        "claude-code1 serverClaude Code",
        "claude-desktop1 serverfolder routingsame as claude-code",
      ]),
    );
  });

  it("test11_servers_access_sets_flags_a_server_no_set_reaches", async () => {
    setup([{ id: "work", name: "Work", serverIds: ["srv-alpha"] }], undefined, "work");
    const table = await screen.findByRole("table");
    expect(await within(table).findByText("No set reaches it")).toBeInTheDocument();
    expect(within(table).getByText("beta-crm").closest("tr")).toHaveTextContent(
      "No set reaches it",
    );
    expect(within(table).getByText("alpha-api").closest("tr")).not.toHaveTextContent(
      "No set reaches it",
    );
    expect(within(table).getByText("gamma-docs").closest("tr")).not.toHaveTextContent(
      "No set reaches it",
    );
    expect(
      await screen.findByText(/No access set reaches beta-crm: the default access/),
    ).toBeInTheDocument();
  });

  it("test12_servers_access_sets_explains_a_folder_mapping_that_outranks_a_clients_set", async () => {
    setup(
      [
        { id: "work", name: "Work", serverIds: ["srv-alpha"] },
        { id: "bare", name: "Bare", serverIds: ["srv-alpha"] },
        { id: "proj", name: "Proj", serverIds: ["srv-alpha", "srv-beta"] },
      ],
      {
        clients: [{ id: "claude-code", name: "Claude Code", set: "bare" }],
        folders: [{ path: "/work/proj", set: "proj" }],
      },
      "work",
    );
    const findings = await screen.findByRole("list", { name: "Access findings" });
    expect(within(findings).getByText("Folder wins")).toBeInTheDocument();
    expect(findings).toHaveTextContent(
      "In /work/proj the gateway uses Proj, not Claude Code's own set: it adds 1 server its own set leaves out (beta-crm). A folder mapping outranks a client's own access set",
    );
    expect(screen.getByText("Folder routing")).toBeInTheDocument();
    expect(screen.getByText("/work/proj")).toBeInTheDocument();
  });
});
