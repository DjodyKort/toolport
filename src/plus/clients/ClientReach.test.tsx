import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DetectedClient } from "@/lib/types";
import type { ReachWorldOptions } from "../fixtures/reachWorld";
import { resetPlusFixture } from "../fixtures/ipc";
import { stdioServer } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { PLUS_SETTINGS_EVENT } from "../shell/navigate";
import ClientReach from "./ClientReach";
import { clientReach } from "./reachModel";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

const COVERS = ["client.reach-panel"] as const;

const servers = [
  stdioServer("srv-alpha", "alpha-api"),
  stdioServer("srv-beta", "beta-crm"),
  stdioServer("srv-gamma", "gamma-docs"),
  stdioServer("srv-delta", "delta-sql"),
  stdioServer("srv-off", "off-mail", { enabled: false }),
];
const sets = [
  { id: "base", name: "Base", serverIds: ["srv-alpha"] },
  { id: "focus", name: "Focus", serverIds: ["srv-alpha", "srv-beta", "srv-gamma"] },
  { id: "mirror", name: "Mirror", serverIds: ["srv-alpha"] },
];

const client = (id: string, name: string) =>
  ({
    id,
    name,
    usesConnectors: false,
    configPath: "/Users/example/.claude.json",
    configExists: true,
    appPresent: true,
    servers: [],
    pluginServers: [],
    gatewayInstalled: true,
    entryState: "managed",
    error: null,
  }) as unknown as DetectedClient;

function setup(id: string, name: string, reach: ReachWorldOptions = {}) {
  resetPlusFixture({
    servers: { servers, sets, defaultAccess: "base" },
    reach: {
      clients: [
        { id: "claude-code", name: "Claude Code", set: "focus" },
        { id: "claude-desktop", name: "Claude Desktop" },
        { id: "cursor", name: "Cursor", set: "mirror" },
        { id: "zed", name: "Zed", set: "gone" },
      ],
      ...reach,
    },
  });
  render(<ClientReach client={client(id, name)} />);
}

beforeEach(() => vi.clearAllMocks());

describe("ClientReach", () => {
  it("test01_names_every_action_it_covers_as_built", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id]).toMatchObject({
        status: "built",
        test: "src/plus/clients/ClientReach.test.tsx",
      });
    }
  });

  it("test02_lists_what_the_set_reaches_and_what_it_leaves_out", async () => {
    setup("claude-code", "Claude Code");
    expect(
      await screen.findByText(
        "Access set Focus · reaches 3 of the 4 servers that are on",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Through Toolport")).toBeInTheDocument();
    const reaches = await screen.findByRole("list", { name: "Reaches" });
    expect(
      within(reaches)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["alpha-api", "beta-crm", "gamma-docs"]);
    const left = screen.getByRole("list", { name: "Does not reach" });
    expect(within(left).getByText("delta-sql", { exact: false })).toBeInTheDocument();
    expect(within(left).queryByText(/off-mail/)).toBeNull();
  });

  it("test03_a_server_no_set_reaches_carries_the_attention_wording", async () => {
    setup("claude-code", "Claude Code");
    const left = await screen.findByRole("list", { name: "Does not reach" });
    expect(within(left).getByText("No access set reaches it")).toBeInTheDocument();
  });

  it("test04_a_server_other_sets_reach_is_not_called_unreached", async () => {
    setup("cursor", "Cursor", {
      folders: [],
      clients: [
        { id: "cursor", name: "Cursor", set: "mirror" },
        { id: "claude-code", name: "Claude Code", set: "focus" },
      ],
    });
    const left = await screen.findByRole("list", { name: "Does not reach" });
    const beta = within(left).getByText(/^beta-crm/);
    expect(beta.textContent).toBe("beta-crm");
  });

  it("test05_a_client_on_the_default_access_says_so_and_stays_closed_when_nothing_is_left_out", async () => {
    resetPlusFixture({
      servers: { servers: servers.slice(0, 2), sets, defaultAccess: null },
      reach: { clients: [{ id: "claude-desktop", name: "Claude Desktop" }] },
    });
    render(<ClientReach client={client("claude-desktop", "Claude Desktop")} />);
    expect(
      await screen.findByText(
        "Follows the default access · reaches 2 of the 2 servers that are on",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Reaches" })).toBeNull();
  });

  it("test06_an_own_set_with_the_default_list_says_it_is_the_same_list", async () => {
    setup("cursor", "Cursor");
    expect(
      await screen.findByText(/Same list as the default access/),
    ).toBeInTheDocument();
  });

  it("test07_a_folder_that_outranks_the_set_shows_the_finding_text", async () => {
    setup("claude-code", "Claude Code", {
      folders: [{ path: "/work/acme", set: "base" }],
    });
    expect(
      await screen.findByText(
        /In \/work\/acme the gateway uses Base, not Claude Code's own set/,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/A folder mapping outranks a client's own access set/),
    ).toBeInTheDocument();
  });

  it("test08_a_client_the_report_does_not_list_has_no_gateway_entry", async () => {
    setup("windsurf", "Windsurf");
    expect(
      await screen.findByText(
        "No Toolport gateway entry in its config file, so no access set applies to it",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Not connected")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Reaches" })).toBeNull();
  });

  it("test09_a_deleted_set_reaches_nothing_and_says_why", async () => {
    setup("zed", "Zed");
    expect(
      await screen.findByText(
        "Zed is bound to an access set that no longer exists, so it sees no servers.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Set deleted")).toBeInTheDocument();
    const left = await screen.findByRole("list", { name: "Does not reach" });
    expect(within(left).getAllByRole("listitem")).toHaveLength(4);
  });

  it("test10_compare_access_sets_opens_the_table_on_the_access_page", async () => {
    setup("claude-code", "Claude Code");
    const opened = vi.fn();
    const listener = (event: Event) => opened((event as CustomEvent).detail);
    window.addEventListener(PLUS_SETTINGS_EVENT, listener);
    try {
      await userEvent.click(
        await screen.findByRole("button", { name: /Compare access sets/ }),
      );
      expect(opened).toHaveBeenCalledWith({
        page: "access",
        section: "Compare access sets",
      });
    } finally {
      window.removeEventListener(PLUS_SETTINGS_EVENT, listener);
    }
  });

  it("test11_a_failed_read_says_so_and_keeps_the_page", async () => {
    resetPlusFixture({ servers: { servers, sets }, down: true });
    render(<ClientReach client={client("claude-code", "Claude Code")} />);
    expect(
      await screen.findByText(/Could not read who reaches what/),
    ).toBeInTheDocument();
  });

  it("test12_the_model_waits_for_the_report", () => {
    expect(clientReach(null, "claude-code")).toBeNull();
  });
});
