import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { setsFromIds, stdioServer, type WorldServer } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import ServerOperations from "./ServerOperations";
import ServersBanner, { ServersListEnd } from "./ServersBanner";
import { resetRestartStore, restart } from "./restartStore";
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

const COVERS = ["servers.list", "servers.add", "servers.add-catalog"] as const;

const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };
const alpha = stdioServer("srv-alpha", "alpha-api");
const beta = stdioServer("srv-beta", "beta-crm");
const notes = (extra: Partial<WorldServer> = {}) =>
  stdioServer("srv-notes", "november-notes", {
    env: [{ key: "NOTES_API_KEY", secret: true }],
    ...extra,
  });
const health = {
  serverId: "srv-notes",
  ok: true,
  toolCount: 11,
  error: null,
  authRequired: false,
};

function world(
  servers: WorldServer[],
  extra: {
    defaultAccess?: string | null;
    vault?: string[];
    restart?: Record<string, { kind: "partial" }>;
    sets?: Array<{ id: string; name: string; serverIds: string[] }>;
  } = {},
) {
  const sets = extra.sets ?? setsFromIds(["local", "work"], servers);
  resetPlusFixture({
    servers: {
      servers,
      sets,
      defaultAccess: extra.defaultAccess ?? null,
      vault: extra.vault,
      restart: extra.restart,
    },
  });
  getRegistryMock.mockResolvedValue(registryOf(servers, sets));
  return sets;
}

beforeEach(() => {
  getRegistryMock.mockReset();
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  vi.clearAllMocks();
});

describe("Servers list rows", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/servers/ServersBanner.test.tsx",
      });
    }
  });

  it("test01_servers_list_ends_with_what_is_on_and_who_sees_it_by_default", async () => {
    const sets = world([alpha, { ...beta, enabled: false }], { defaultAccess: "work" });
    render(
      <ServersListEnd
        registry={registryOf([alpha, { ...beta, enabled: false }], sets, {
          defaultAccess: "work",
        })}
        host={host}
      />,
    );
    const line = await screen.findByText(/1 server on, 1 off/);
    expect(line.parentElement).toHaveTextContent(
      "Clients without their own access set see the set Work",
    );
    expect(ranArgv()).toContainEqual(["server", "ls"]);
    await userEvent.click(screen.getByRole("button", { name: /Compare access sets/ }));
    expect(host.openAccessSets).toHaveBeenCalled();
  });

  it("test02_servers_list_without_a_default_set_says_every_enabled_server", async () => {
    const sets = world([alpha, beta]);
    render(<ServersListEnd registry={registryOf([alpha, beta], sets)} host={host} />);
    expect(
      await screen.findByText(
        /Clients without their own access set see every enabled server/,
      ),
    ).toBeInTheDocument();
  });

  it("test03_servers_list_adds_no_row_when_nothing_needs_attention", () => {
    const sets = world([alpha, beta]);
    const { container } = render(
      <ServersBanner registry={registryOf([alpha, beta], sets)} host={host} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("test04_servers_list_shows_a_failed_restart_with_the_server_name_and_a_retry", async () => {
    const sets = world([alpha, beta], { restart: { "srv-alpha": { kind: "partial" } } });
    render(<ServersBanner registry={registryOf([alpha, beta], sets)} host={host} />);
    await restart("srv-alpha", "alpha-api");
    expect(
      await screen.findByText("A restart failed in some gateways"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open alpha-api" }));
    expect(host.openServer).toHaveBeenCalledWith("srv-alpha");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(ranArgv().filter((a) => a[1] === "restart")).toHaveLength(2),
    );
  });
});

describe("Add a server", () => {
  function addNotes(extra: Partial<WorldServer> = {}) {
    const before = [alpha, beta];
    const after = [alpha, beta, notes(extra)];
    world(before);
    const view = (list: WorldServer[]) => (
      <ServersBanner
        registry={registryOf(list, setsFromIds(["local", "work"], list))}
        host={host}
      />
    );
    const { rerender } = render(view(before));
    world(after);
    rerender(view(after));
    return { after };
  }

  it("test05_servers_add_names_a_server_that_appeared_and_opens_it_from_the_list", async () => {
    addNotes();
    expect(await screen.findByText("1 new server has setup left")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open november-notes" }));
    expect(host.openServer).toHaveBeenCalledWith("srv-notes");
  });

  it("test06_servers_add_does_not_flag_the_servers_that_were_there_at_the_first_sight", () => {
    const { container } = render(
      <ServersBanner
        registry={registryOf(
          [alpha, beta],
          setsFromIds(["local", "work"], [alpha, beta]),
        )}
        host={host}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  async function openNew(extra: Partial<WorldServer> = {}, vault: string[] = []) {
    const { after } = addNotes(extra);
    await screen.findByText("1 new server has setup left");
    const sets = setsFromIds(["local", "work"], after);
    world(after, { vault, sets: [sets[0], { ...sets[1], serverIds: ["srv-alpha"] }] });
    const registry = registryOf(
      after,
      [sets[0], { ...sets[1], serverIds: ["srv-alpha"] }],
      {
        scopes: { cursor: "work", "claude-desktop": "work" },
      },
    );
    getRegistryMock.mockResolvedValue(registry);
    render(
      <ServerOperations
        server={entryOf(after[2])}
        registry={registry}
        health={health}
        host={host}
        onRegistryChange={vi.fn()}
      />,
    );
    return screen.findByRole("region", { name: "Finish setup" });
  }

  it("test07_servers_add_finish_setup_lists_what_is_missing_and_closes_what_is_true", async () => {
    const finish = await openNew();
    expect(await within(finish).findByText("2 things left")).toBeInTheDocument();
    expect(within(finish).getByText("It starts")).toBeInTheDocument();
    expect(within(finish).getByText("Give it its secret")).toBeInTheDocument();
    expect(
      within(finish).getByText(/NOTES_API_KEY is declared but empty/),
    ).toBeInTheDocument();
    expect(
      within(finish).getByRole("button", { name: "Set NOTES_API_KEY…" }),
    ).toBeInTheDocument();
    expect(
      within(finish).getByText(
        /Cursor, Claude Desktop use narrower access sets and cannot see it yet/,
      ),
    ).toBeInTheDocument();
    await userEvent.click(within(finish).getByRole("button", { name: "Add to 1 set" }));
    await waitFor(() =>
      expect(within(finish).queryByText("Who can reach it")).not.toBeInTheDocument(),
    );
    expect(within(finish).getByText("1 thing left")).toBeInTheDocument();
  });

  it("test08_servers_add_catalog_setup_closes_itself_when_every_fact_is_true", async () => {
    const finish = await openNew({ source: "curated" }, ["srv-notes/NOTES_API_KEY"]);
    await within(finish).findByText("Who can reach it");
    await userEvent.click(within(finish).getByRole("button", { name: "Add to 1 set" }));
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("november-notes is set up"),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("region", { name: "Finish setup" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("test09_servers_add_catalog_not_now_closes_the_list_and_keeps_the_facts_in_operations", async () => {
    const finish = await openNew({ source: "curated" });
    await userEvent.click(within(finish).getByRole("button", { name: "Not now" }));
    expect(
      screen.queryByRole("region", { name: "Finish setup" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Operations" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Remove…" })).toBeInTheDocument();
  });

  it("test10_servers_add_finish_setup_names_a_server_that_does_not_start", async () => {
    addNotes();
    const sets = setsFromIds(["local", "work"], [alpha, beta, notes()]);
    world([alpha, beta, notes()], { sets });
    const registry = registryOf([alpha, beta, notes()], sets);
    render(
      <ServerOperations
        server={entryOf(notes())}
        registry={registry}
        health={{
          serverId: "srv-notes",
          ok: false,
          toolCount: 0,
          error: "spawn npx ENOENT",
          authRequired: false,
        }}
        host={host}
        onRegistryChange={vi.fn()}
      />,
    );
    const finish = await screen.findByRole("region", { name: "Finish setup" });
    expect(within(finish).getByText("It does not start")).toBeInTheDocument();
    expect(
      within(finish).getByText("Command or file not found (ENOENT)"),
    ).toBeInTheDocument();
    expect(
      within(finish).getByRole("button", { name: /Edit command/ }),
    ).toBeInTheDocument();
    expect(within(finish).getByRole("button", { name: "Restart" })).toBeInTheDocument();
  });
});
