import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import type { DetectedClient } from "@/lib/types";
import { plusFixtureWorld, resetPlusFixture } from "../fixtures/ipc";
import type { DirectWorldOptions } from "../fixtures/directWorld";
import { stdioServer } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import DirectEntries from "./DirectEntries";
import { STATES } from "./model";
import type { DirectState } from "./data";

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
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const COVERS = ["client.direct-entries"] as const;

const servers = [
  stdioServer("srv-alpha", "alpha-api", { env: [{ key: "API_KEY", secret: true }] }),
  stdioServer("srv-beta", "beta-crm"),
  stdioServer("srv-sierra", "sierra-design", {
    transport: "http",
    command: null,
    url: "https://example.test/mcp",
  }),
];

const client = (extra: Partial<DetectedClient> = {}): DetectedClient =>
  ({
    id: "claude-code",
    name: "Claude Code",
    usesConnectors: false,
    configPath: "/Users/example/.claude.json",
    configExists: true,
    appPresent: true,
    servers: [],
    pluginServers: [],
    gatewayInstalled: true,
    entryState: "managed",
    error: null,
    ...extra,
  }) as DetectedClient;

function setup(direct: DirectWorldOptions = {}, extra: Partial<DetectedClient> = {}) {
  resetPlusFixture({ servers: { servers }, direct });
  const onChanged = vi.fn();
  render(<DirectEntries client={client(extra)} onChanged={onChanged} />);
  return { onChanged };
}

const writes = () => plusFixtureWorld().direct.writes;

const planHash = (dialog: HTMLElement) =>
  dialog.querySelector("[data-plan-hash]")?.getAttribute("data-plan-hash");

async function openAdd(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: /Add direct entry/ }));
  return screen.findByRole("dialog");
}

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("DirectEntries", () => {
  it("test01_covers_the_parity_action_as_a_built_one", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id]).toMatchObject({ status: "built" });
    }
  });

  it("test02_an_empty_client_says_it_reaches_servers_through_the_gateway", async () => {
    setup();
    expect(
      await screen.findByText(/reaches your servers through Toolport's one/),
    ).toBeTruthy();
    expect(screen.getByText(/bypass the gateway/)).toBeTruthy();
  });

  it("test03_the_add_dialog_plans_first_and_writes_only_that_plan", async () => {
    const user = userEvent.setup();
    const { onChanged } = setup();
    const dialog = await openAdd(user);
    const confirm = within(dialog).getByRole("button", { name: "Add entry" });
    expect(confirm).toHaveProperty("disabled", true);

    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-alpha");
    await within(dialog).findByText(/Change to/);
    expect(within(dialog).getByText(/toolportctl direct run srv-alpha/)).toBeTruthy();
    expect(within(dialog).getByText(/bypasses the Toolport gateway/)).toBeTruthy();
    const hash = planHash(dialog);
    expect(writes()).toHaveLength(0);
    expect(ranArgv().at(-1)).toContain("--dry-run");

    await user.click(confirm);
    await waitFor(() => expect(writes()).toHaveLength(1));
    expect(writes()[0]).toEqual(
      expect.arrayContaining([
        "add",
        "srv-alpha",
        "--client",
        "claude-code",
        "--plan-hash",
      ]),
    );
    expect(writes()[0]).toContain(hash);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(await screen.findByText("alpha-api")).toBeTruthy();
    expect(onChanged).toHaveBeenCalled();
  });

  it("test04_undo_in_the_toast_removes_the_entry_it_added", async () => {
    const user = userEvent.setup();
    setup();
    const dialog = await openAdd(user);
    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-beta");
    await within(dialog).findByText(/Change to/);
    await user.click(within(dialog).getByRole("button", { name: "Add entry" }));
    await waitFor(() => expect(toast.success).toHaveBeenCalled());
    const [, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [
      string,
      { action: { label: string; onClick: () => void }; duration: number },
    ];
    expect(options.action.label).toBe("Undo");
    expect(options.duration).toBe(10000);

    options.action.onClick();
    await waitFor(() => expect(writes()).toHaveLength(2));
    expect(writes()[1]).toEqual(expect.arrayContaining(["rm", "srv-beta"]));
    expect(plusFixtureWorld().direct.entries).toHaveLength(0);
  });

  it("test05_a_refused_server_shows_why_and_cannot_be_added", async () => {
    const user = userEvent.setup();
    setup({
      refuse: { "srv-alpha": "its working directory follows the client's project root" },
    });
    const dialog = await openAdd(user);
    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-alpha");
    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "cannot get a direct entry",
    );
    expect(within(dialog).getByRole("button", { name: "Add entry" })).toHaveProperty(
      "disabled",
      true,
    );
  });

  it("test06_a_client_that_cannot_hold_an_entry_says_so_in_the_dialog", async () => {
    const user = userEvent.setup();
    setup({
      clients: [
        {
          id: "claude-code",
          name: "Claude Code",
          path: "/Users/example/.claude.json",
          unsupported: "Claude Code cannot hold a direct entry: its file is YAML",
        },
      ],
    });
    const dialog = await openAdd(user);
    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-beta");
    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "cannot hold a direct entry",
    );
  });

  it("test07_a_remote_server_is_written_by_its_address_only", async () => {
    const user = userEvent.setup();
    setup();
    const dialog = await openAdd(user);
    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-sierra");
    await within(dialog).findByText(/Change to/);
    expect(within(dialog).getByText(/By its address/)).toBeTruthy();
    expect(within(dialog).getAllByText(/signs in to the server by itself/)).toHaveLength(
      2,
    );
    expect(within(dialog).queryByText(/direct run/)).toBeNull();
  });

  it("test08_a_plan_that_went_stale_is_refused_and_planned_again", async () => {
    const user = userEvent.setup();
    setup();
    const dialog = await openAdd(user);
    await user.selectOptions(within(dialog).getByLabelText("Server"), "srv-beta");
    await within(dialog).findByText(/Change to/);
    const before = planHash(dialog);
    plusFixtureWorld().direct.touch();

    await user.click(within(dialog).getByRole("button", { name: "Add entry" }));
    await waitFor(() =>
      expect(vi.mocked(toast.error).mock.calls.length).toBeGreaterThan(0),
    );
    expect(writes()).toHaveLength(0);
    await waitFor(() => expect(planHash(dialog)).not.toBe(before));
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("test09_every_state_shows_its_label_its_meaning_and_what_it_offers", async () => {
    const states: Array<[DirectState, string[]]> = [
      ["ok", ["Remove entry"]],
      ["stale", ["Rewrite", "Remove entry"]],
      ["customized", ["Rewrite", "Remove entry"]],
      ["missing", ["Rewrite", "Forget"]],
      ["orphan", ["Remove entry"]],
      ["relocated", ["Re-point", "Forget"]],
    ];
    setup(
      {
        entries: states.map(([state], at) => ({
          client: "claude-code",
          entry: `entry-${state}`,
          server: at === 0 ? "srv-alpha" : "srv-beta",
          state,
        })),
      },
      {
        servers: states.map(([state]) => ({
          name: `entry-${state}`,
          transport: "stdio",
          command: "x",
          args: [],
          envKeys: [],
          url: null,
        })),
      },
    );
    for (const [state, offers] of states) {
      const row = await waitFor(() => {
        const found = document.querySelector(`[data-operation="entry-${state}"]`);
        if (!found) throw new Error("not yet");
        return found as HTMLElement;
      });
      expect(within(row).getByText(STATES[state].label)).toBeTruthy();
      if (state !== "ok")
        expect(within(row).getByText(STATES[state].meaning)).toBeTruthy();
      const buttons = within(row)
        .getAllByRole("button")
        .map((b) => b.textContent)
        .filter((text) => text && text !== "Copy path");
      expect(buttons).toEqual(offers);
    }
  });

  it("test10_an_untracked_entry_is_offered_to_be_tracked_and_keeps_the_client_file", async () => {
    const user = userEvent.setup();
    setup({
      entries: [
        {
          client: "claude-code",
          entry: "beta-crm",
          server: "srv-beta",
          state: "unrecorded",
        },
      ],
    });
    const row = await waitFor(() => {
      const found = document.querySelector('[data-operation="beta-crm"]');
      if (!found) throw new Error("not yet");
      return found as HTMLElement;
    });
    expect(within(row).getByText("Not tracked")).toBeTruthy();
    await user.click(within(row).getByRole("button", { name: "Track it beta-crm" }));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Plan plan-/);
    await user.click(within(dialog).getByRole("button", { name: "Track entry" }));
    await waitFor(() => expect(writes()).toHaveLength(1));
    expect(writes()[0]).toEqual(expect.arrayContaining(["add", "srv-beta"]));
    expect(toast.success).toHaveBeenCalledWith(
      "Now tracking beta-crm",
      expect.objectContaining({ action: undefined }),
    );
  });

  it("test11_rewriting_an_edited_entry_asks_for_force_and_shows_the_plan_first", async () => {
    const user = userEvent.setup();
    setup({
      entries: [
        {
          client: "claude-code",
          entry: "alpha-api",
          server: "srv-alpha",
          state: "customized",
        },
      ],
    });
    await user.click(await screen.findByRole("button", { name: "Rewrite alpha-api" }));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Change to/);
    expect(writes()).toHaveLength(0);
    expect(ranArgv().at(-1)).toEqual(expect.arrayContaining(["--force", "--dry-run"]));
    await user.click(within(dialog).getByRole("button", { name: "Rewrite entry" }));
    await waitFor(() => expect(writes()).toHaveLength(1));
    expect(writes()[0]).toContain("--force");
    expect(plusFixtureWorld().direct.entries[0].state).toBe("ok");
  });

  it("test12_a_missing_entry_is_forgotten_without_touching_the_client_file_and_forget_is_undoable", async () => {
    const user = userEvent.setup();
    setup({
      entries: [
        {
          client: "claude-code",
          entry: "beta-crm",
          server: "srv-beta",
          state: "missing",
        },
      ],
    });
    await user.click(await screen.findByRole("button", { name: "Forget beta-crm" }));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/drop the record of 'beta-crm'/);
    expect(within(dialog).queryByText(/Change to/)).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Forget" }));
    await waitFor(() => expect(plusFixtureWorld().direct.entries).toHaveLength(0));
    const [, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [
      string,
      { action: { label: string } },
    ];
    expect(options.action.label).toBe("Undo");
  });

  it("test13_an_orphan_whose_entry_is_gone_offers_forget_and_one_still_there_offers_remove", async () => {
    setup(
      {
        entries: [
          {
            client: "claude-code",
            entry: "gone-one",
            server: "srv-beta",
            state: "orphan",
          },
          {
            client: "claude-code",
            entry: "still-there",
            server: "srv-beta",
            state: "orphan",
          },
        ],
      },
      {
        servers: [
          {
            name: "still-there",
            transport: "stdio",
            command: "x",
            args: [],
            envKeys: [],
            url: null,
          },
        ],
      },
    );
    const gone = await waitFor(() => {
      const found = document.querySelector('[data-operation="gone-one"]');
      if (!found) throw new Error("not yet");
      return found as HTMLElement;
    });
    expect(within(gone).getByRole("button", { name: "Forget gone-one" })).toBeTruthy();
    expect(within(gone).queryByRole("button", { name: /Remove entry/ })).toBeNull();
    const there = document.querySelector('[data-operation="still-there"]') as HTMLElement;
    expect(
      within(there).getByRole("button", { name: "Remove entry still-there" }),
    ).toBeTruthy();
  });

  it("test14_entries_of_the_clients_own_file_are_listed_as_not_managed", async () => {
    setup(
      {},
      {
        servers: [
          {
            name: "legacy-lint",
            transport: "stdio",
            command: "x",
            args: [],
            envKeys: [],
            url: null,
          },
        ],
      },
    );
    const row = await waitFor(() => {
      const found = document.querySelector('[data-operation="legacy-lint"]');
      if (!found) throw new Error("not yet");
      return found as HTMLElement;
    });
    expect(within(row).getByText("Not managed")).toBeTruthy();
    expect(within(row).queryAllByRole("button")).toHaveLength(0);
  });

  it("test15_the_add_dialog_opens_and_closes_from_the_keyboard", async () => {
    const user = userEvent.setup();
    setup();
    const button = await screen.findByRole("button", { name: /Add direct entry/ });
    button.focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog")).toBeTruthy();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toHaveLength(0);
  });

  it("test16_a_client_that_is_not_installed_shows_no_panel", () => {
    setup({}, { configExists: false, appPresent: false });
    expect(screen.queryByText("Direct entries")).toBeNull();
  });

  it("test17_a_read_that_fails_says_so_and_offers_another_try", async () => {
    resetPlusFixture({ servers: { servers }, down: true });
    render(<DirectEntries client={client()} onChanged={vi.fn()} />);
    expect(await screen.findByText(/could not be read/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  });
});
