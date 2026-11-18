import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import type { WorldOptions } from "../fixtures/ctlWorld";
import { resetPlusFixture } from "../fixtures/ipc";
import { setsFromIds, stdioServer, type WorldServer } from "../fixtures/serverWorld";
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

const COVERS = ["servers.detail", "servers.edit", "servers.remove"] as const;

const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

const alpha: WorldServer = stdioServer("srv-alpha", "alpha-api", {
  command: "uv",
  args: ["run", "alpha-api", "--stdio"],
  cwd: "~/work/alpha-api",
  env: [
    { key: "API_KEY", secret: true },
    { key: "REGION", secret: false },
  ],
});
const beta = stdioServer("srv-beta", "beta-crm");

function setup(
  options: {
    servers?: WorldServer[];
    defaultAccess?: string | null;
    sets?: Array<{ id: string; name: string; serverIds: string[] }>;
    reach?: WorldOptions["reach"];
  } = {},
) {
  const servers = options.servers ?? [alpha, beta];
  const sets = options.sets ?? setsFromIds(["local", "work"], servers);
  resetPlusFixture({
    servers: { servers, sets, defaultAccess: options.defaultAccess ?? null },
    reach: options.reach,
  });
  const registry = registryOf(servers, sets, { defaultAccess: options.defaultAccess });
  getRegistryMock.mockResolvedValue(registry);
  const onRegistryChange = vi.fn();
  const view = (reg = registry, server = servers[0]) => (
    <ServerOperations
      server={entryOf(server)}
      registry={reg}
      host={host}
      onRegistryChange={onRegistryChange}
    />
  );
  const result = render(view());
  return { ...result, view, registry, servers, sets, onRegistryChange };
}

const open = (name: RegExp) => userEvent.click(screen.getByRole("button", { name }));

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});

describe("Operations list", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/servers/ServerOperations.test.tsx",
      });
    }
  });

  it("test01_servers_detail_lists_runtime_reach_and_remove_under_one_header", async () => {
    setup();
    const card = await screen.findByRole("region", { name: "Operations" });
    expect(
      within(card)
        .getAllByText(/^(Runtime|Who can reach it|Remove and clean up)$/)
        .map((node) => node.textContent),
    ).toEqual(["Runtime", "Who can reach it", "Remove and clean up"]);
    expect(within(card).queryByText(/need you/)).not.toBeInTheDocument();
  });

  it("test02_servers_detail_says_what_the_process_sees_and_names_its_env_keys", async () => {
    setup();
    await open(/^Runtime/);
    expect(await screen.findByText("uv run alpha-api --stdio")).toBeInTheDocument();
    expect(screen.getByText("~/work/alpha-api")).toBeInTheDocument();
    expect(screen.getByText(/only a fixed set \(PATH, HOME/)).toBeInTheDocument();
    expect(screen.getByText(/Use my shell environment/)).toBeInTheDocument();
    const badge = (key: string) =>
      screen
        .getAllByText(new RegExp(key))
        .map((node) => node.closest("[data-slot=badge]"))
        .find(Boolean);
    expect(badge("API_KEY")).toHaveTextContent("API_KEY · secret");
    expect(badge("REGION")).toHaveTextContent("REGION · plain");
    expect(invokeLog.some((e) => e.command === "plus_ctl")).toBe(true);
    expect(ranArgv()).toContainEqual(["server", "info", "srv-alpha"]);
  });

  it("test03_servers_detail_with_the_shell_environment_on_says_so_and_adds_no_hint", async () => {
    setup({ servers: [{ ...alpha, inheritEnv: true }, beta] });
    await open(/^Runtime/);
    expect(await screen.findByText(/your whole shell environment/)).toBeInTheDocument();
    expect(screen.queryByText(/Use my shell environment/)).not.toBeInTheDocument();
  });

  it("test04_servers_edit_is_upstreams_dialog_and_the_runtime_row_notes_the_change", async () => {
    const { view, servers, rerender } = setup();
    expect(
      await screen.findByText(/A restart starts a fresh process/),
    ).toBeInTheDocument();
    const edited = [{ ...servers[0], args: ["run", "alpha-api", "--http"] }, beta];
    rerender(view(registryOf(edited, setsFromIds(["local", "work"], edited)), edited[0]));
    expect(
      await screen.findByText(
        /Settings changed \d\d:\d\d.* · a new process starts on the next call/,
      ),
    ).toBeInTheDocument();
  });

  it("test05_servers_remove_previews_before_it_removes_and_lists_the_vault_entries", async () => {
    const { onRegistryChange } = setup();
    await userEvent.click(await screen.findByRole("button", { name: "Remove…" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove alpha-api?" });
    expect(await within(dialog).findByText("Vault entries: API_KEY")).toBeInTheDocument();
    expect(within(dialog).getByText(/No client entries/)).toBeInTheDocument();
    expect(ranArgv()).toContainEqual(["server", "uninstall", "srv-alpha", "--dry-run"]);
    expect(
      ranArgv().filter((a) => a[1] === "uninstall" && !a.includes("--dry-run")),
    ).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove server" }));
    await waitFor(() => expect(host.closeServer).toHaveBeenCalled());
    expect(ranArgv()).toContainEqual(["server", "uninstall", "srv-alpha"]);
    expect(onRegistryChange).toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith("Removed alpha-api and 1 vault entry");
  });

  it("test06_servers_remove_keeps_the_vault_and_says_keep_client_entries_leaves_orphans", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Remove…" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove alpha-api?" });
    expect(
      within(dialog).getByText(/show as orphans until you remove them/),
    ).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("checkbox", { name: /Also remove its vault entries/ }),
    );
    await userEvent.click(
      within(dialog).getByRole("checkbox", { name: /Keep client entries/ }),
    );
    expect(await within(dialog).findByText("No vault entries")).toBeInTheDocument();
    expect(
      await within(dialog).findByText(/Direct entries stay in the clients' files/),
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove server" }));
    await waitFor(() => expect(host.closeServer).toHaveBeenCalled());
    expect(ranArgv()).toContainEqual([
      "server",
      "uninstall",
      "srv-alpha",
      "--keep-secrets",
      "--keep-clients",
    ]);
    expect(toast.success).toHaveBeenCalledWith("Removed alpha-api");
  });

  it("test07_a_server_without_secrets_offers_the_plain_remove_row_and_no_vault_choice", async () => {
    setup({ servers: [beta, alpha] });
    await userEvent.click(await screen.findByRole("button", { name: "Remove…" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove beta-crm?" });
    expect(
      within(dialog).queryByRole("checkbox", { name: /Also remove its vault entries/ }),
    ).not.toBeInTheDocument();
    expect(within(dialog).getByText("No vault entries")).toBeInTheDocument();
  });

  it("test08_a_server_the_default_access_leaves_out_is_flagged_and_added_with_one_click", async () => {
    const { servers, onRegistryChange } = setup({
      defaultAccess: "work",
      sets: [
        { id: "local", name: "Local fixture", serverIds: ["srv-alpha", "srv-beta"] },
        { id: "work", name: "Work", serverIds: ["srv-beta"] },
      ],
      reach: { clients: [{ id: "claude-code", set: "local" }] },
    });
    expect(servers).toHaveLength(2);
    expect(await screen.findByText("1 need you")).toBeInTheDocument();
    expect(
      await screen.findByText(/Clients on the default access \(Work\) cannot see it/),
    ).toBeInTheDocument();
    const toggle = await screen.findByRole("button", { name: "alpha-api in Work" });
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "alpha-api in Work" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    const write = invokeLog.find(
      (e) =>
        e.command === "plus_ctl" &&
        (e.args as { argv: string[] }).argv[2] === "servers_add_profile_tag",
    );
    expect(JSON.parse((write!.args as { stdinSecret: string }).stdinSecret)).toEqual({
      name: "srv-alpha",
      profile_tag: "work",
    });
    const next = onRegistryChange.mock.calls.at(-1)![0];
    expect(
      next.profiles.find((p: { id: string }) => p.id === "work").enabledServerIds,
    ).toEqual(["srv-beta", "srv-alpha"]);
    expect(screen.queryByText("1 need you")).not.toBeInTheDocument();
  });

  it("test09_a_switched_off_server_says_its_sets_are_kept_and_nothing_reaches_it", async () => {
    setup({ servers: [{ ...alpha, enabled: false }, beta] });
    expect(
      await screen.findByText(
        /Switched off · no client reaches it, and its access sets are kept/,
      ),
    ).toBeInTheDocument();
  });

  it("test10_a_server_no_access_set_reaches_says_so_and_opens_the_row", async () => {
    setup({
      defaultAccess: "work",
      sets: [
        { id: "local", name: "Local fixture", serverIds: ["srv-alpha", "srv-beta"] },
        { id: "work", name: "Work", serverIds: ["srv-beta"] },
      ],
    });
    expect(await screen.findByText("Nobody reaches it")).toBeInTheDocument();
    expect(
      screen.getByText(
        /No access set reaches it: the default access, the clients' own sets/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /No client, folder or default access reaches it through an access set/,
      ),
    ).toBeInTheDocument();
    expect(ranArgv()).toContainEqual(["client", "reach"]);
  });

  it("test11_the_reach_row_names_the_clients_and_folders_that_reach_the_server", async () => {
    setup({
      defaultAccess: "work",
      sets: [
        { id: "local", name: "Local fixture", serverIds: ["srv-alpha", "srv-beta"] },
        { id: "work", name: "Work", serverIds: ["srv-beta"] },
      ],
      reach: {
        clients: [{ id: "claude-code", name: "Claude Code", set: "local" }],
        folders: [{ path: "/work/proj", set: "local" }],
        direct: [{ client: "cursor", server: "srv-alpha", state: "stale" }],
      },
    });
    expect(
      await screen.findByText(/Reached by Claude Code, folder \/work\/proj\./),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Direct entries bypass the sets: Cursor \(stale\)/),
    ).toBeInTheDocument();
    expect(screen.queryByText("Nobody reaches it")).not.toBeInTheDocument();
  });
});
