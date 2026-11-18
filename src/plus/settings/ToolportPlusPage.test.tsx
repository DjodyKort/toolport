import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { commandsGolden } from "../fixtures/commandsGolden";
import toolsGolden from "../../../plus/fixtures/ctl-envelopes/mcp-tools.json";
import ToolportPlusPage from "./ToolportPlusPage";
import { getRegistryMock, registryWithSets } from "./testkit";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  getRegistry: (...args: unknown[]) => getRegistryMock(...args),
}));

const registry = registryWithSets("local", "work");

/** The actions of src/plus/gui-parity/ this file proves: status and doctor lead the page and
 * list the checks, the self-management row connects, removes, shows its checks and tools, the
 * commands page lists the registry and runs a tool through mcp call, and the registry index card
 * reads catalog status and runs catalog sync. */
const COVERS = [
  "system.status",
  "system.doctor",
  "system.mcp-install",
  "system.mcp-uninstall",
  "system.mcp-doctor",
  "system.mcp-tools",
  "catalog.commands",
  "catalog.mcp-call",
  "memory.overview",
  "registry-index.status",
  "registry-index.sync",
] as const;

function renderPage(props: Partial<React.ComponentProps<typeof ToolportPlusPage>> = {}) {
  const onRegistryChange = vi.fn();
  const onOpenServers = vi.fn();
  render(
    <ToolportPlusPage
      registry={registry}
      onRegistryChange={onRegistryChange}
      onOpenServers={onOpenServers}
      {...props}
    />,
  );
  return { onRegistryChange, onOpenServers };
}

beforeEach(() => {
  getRegistryMock.mockReset().mockResolvedValue(registry);
});

describe("Settings > Toolport+", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/settings/ToolportPlusPage.test.tsx",
      });
    }
    expect(
      Object.entries(guiParity.actions)
        .filter(
          ([, action]) =>
            guiParity.routes[action.route]?.component ===
            "src/plus/settings/ToolportPlusPage.tsx",
        )
        .map(([id]) => id)
        .filter((id) => !(COVERS as readonly string[]).includes(id)),
    ).toEqual([]);
  });

  it("test01_leads_with_a_working_state_and_lists_the_installation_checks", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    expect(await screen.findByText("Toolport+ is working")).toBeInTheDocument();
    const installation = screen.getByRole("region", { name: "Installation" });
    expect(within(installation).getByText("Toolport+ build")).toBeInTheDocument();
    expect(within(installation).getAllByText("OK")).toHaveLength(6);
    expect(
      within(installation).getByText(/3 servers, 2 access sets/),
    ).toBeInTheDocument();
  });

  it("test02_first_run_offers_connect_as_the_only_blue_button", async () => {
    resetPlusFixture({ self: "missing" });
    renderPage();
    expect(
      await screen.findByText("Self-management is not connected"),
    ).toBeInTheDocument();
    const connect = screen.getByRole("button", { name: "Connect" });
    expect(connect).toHaveAttribute("data-variant", "default");
    const blue = screen
      .getAllByRole("button")
      .filter((b) => b.getAttribute("data-variant") === "default");
    expect(blue).toEqual([connect]);
  });

  it("test03_connect_runs_mcp_install_and_refreshes_the_apps_registry", async () => {
    resetPlusFixture({ self: "missing" });
    const { onRegistryChange } = renderPage();
    await userEvent.click(await screen.findByRole("button", { name: "Connect" }));
    expect(await screen.findByText("Connected")).toBeInTheDocument();
    expect(getRegistryMock).toHaveBeenCalledTimes(1);
    expect(onRegistryChange).toHaveBeenCalledWith(registry);
    expect(screen.getByText("Toolport+ is working")).toBeInTheDocument();
  });

  it("test04_remove_asks_first_and_then_runs_mcp_uninstall", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: "Remove" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove self-management?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect(await screen.findByText("Removed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect" })).toBeInTheDocument();
  });

  it("test05_a_server_switched_off_in_servers_is_not_switched_back_on", async () => {
    resetPlusFixture({ self: "disabled" });
    const { onOpenServers } = renderPage();
    expect(await screen.findByText("Switched off")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open Servers" }));
    expect(onOpenServers).toHaveBeenCalled();
  });

  it("test06_a_gateway_without_the_fork_seam_leads_the_page", async () => {
    resetPlusFixture({ self: "enabled", doctor: "warn" });
    renderPage();
    expect(
      await screen.findByText("Toolport+ is not fully active in this build"),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/built without the fork seam/)).toHaveLength(2);
  });

  it("test07_an_unreadable_registry_is_the_leading_problem", async () => {
    resetPlusFixture({ self: "enabled", doctor: "fail" });
    renderPage();
    expect(
      await screen.findByText("Toolport cannot read its registry"),
    ).toBeInTheDocument();
    const installation = screen.getByRole("region", { name: "Installation" });
    expect(within(installation).getByText("Problem")).toBeInTheDocument();
  });

  it("test08_a_missing_toolportctl_shows_a_retryable_error_instead_of_an_empty_page", async () => {
    resetPlusFixture({ down: true });
    renderPage();
    expect(
      await screen.findByText("Toolport+ tools are not available"),
    ).toBeInTheDocument();
    expect(screen.getByText(/toolportctl not found/)).toBeInTheDocument();
    resetPlusFixture({ self: "enabled" });
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Toolport+ is working")).toBeInTheDocument();
  });

  it("test09_shows_the_checks_and_tools_behind_the_self_management_status", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Checks \(8\)/ }));
    expect(screen.getByText("Answers a handshake")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", {
        name: new RegExp(
          `Offers ${toolsGolden.envelope.data.tools.length} tools and ${toolsGolden.envelope.data.resources.length} resources`,
        ),
      }),
    );
    expect(screen.getByText("flow_diagram")).toBeInTheDocument();
    expect(screen.getByText("mcpm://paths")).toBeInTheDocument();
  });

  it("test10_all_commands_runs_a_read_only_command_and_copies_the_rest", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Open/ }));
    const commands = await screen.findByRole("region", { name: "Commands" });
    expect(within(commands).getAllByRole("listitem")).toHaveLength(
      commandsGolden.envelope.data.commands.filter((row) => row.kind === "command")
        .length,
    );
    expect(
      within(commands).getByRole("button", { name: "Copy mcp install" }),
    ).toBeInTheDocument();
    expect(
      within(commands).queryByRole("button", { name: "Run mcp install" }),
    ).toBeNull();

    await userEvent.click(within(commands).getByRole("button", { name: "Run doctor" }));
    const dialog = await screen.findByRole("dialog", { name: "toolportctl doctor" });
    await waitFor(() => expect(within(dialog).getByText("Done")).toBeInTheDocument());
    expect(within(dialog).getByLabelText("Output")).toHaveTextContent('"plusBuilt"');
  });

  it("test11_all_commands_filters_commands_and_tools", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Open/ }));
    await userEvent.type(await screen.findByLabelText("Search commands"), "flow");
    expect(screen.getByText('No command matches "flow"')).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run flow_diagram" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Toolport\+/ }));
    expect(await screen.findByText("Toolport+ is working")).toBeInTheDocument();
  });

  it("test12_runs_a_self_management_tool_through_mcp_call", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    await userEvent.click(await screen.findByRole("button", { name: /Open/ }));
    await userEvent.click(await screen.findByRole("button", { name: "Run where_am_i" }));
    const dialog = await screen.findByRole("dialog", {
      name: "toolportctl mcp call where_am_i",
    });
    await userEvent.click(within(dialog).getByRole("button", { name: "Run" }));
    await waitFor(() => expect(within(dialog).getByText("Done")).toBeInTheDocument());
    expect(within(dialog).getByLabelText("Output")).toHaveTextContent('"serverCount": 3');
  });

  it("test20_memory_overview_lists_the_repositories_in_its_own_section", async () => {
    resetPlusFixture({ self: "enabled" });
    renderPage();
    const section = await screen.findByRole("region", { name: "Memory & history" });
    expect(await within(section).findByRole("row", { name: /web-app/ })).toBeVisible();
  });
});

describe("Settings > Toolport+ > Registry index", () => {
  const card = async () =>
    within(await screen.findByRole("region", { name: "Registry index" }));

  it("test20_shows_the_server_count_and_when_the_index_was_synced", async () => {
    resetPlusFixture({ self: "enabled", catalog: { index: "fresh" } });
    renderPage();
    const index = await card();
    expect(await index.findByText("Up to date")).toBeInTheDocument();
    expect(index.getByText("13,712 servers, synced 3 h ago")).toBeInTheDocument();
  });

  it("test21_a_stale_index_says_search_asks_the_live_registry", async () => {
    resetPlusFixture({ self: "enabled", catalog: { index: "stale" } });
    renderPage();
    const index = await card();
    expect(await index.findByText("Out of date")).toBeInTheDocument();
    expect(index.getByText(/ask the live registry/)).toBeInTheDocument();
  });

  it("test22_a_failed_sync_shows_its_reason", async () => {
    resetPlusFixture({ self: "enabled", catalog: { index: "error" } });
    renderPage();
    const index = await card();
    expect(await index.findByText("Sync failed")).toBeInTheDocument();
    expect(index.getByText(/did not answer/)).toBeInTheDocument();
  });

  it("test23_refresh_now_runs_catalog_sync_and_shows_the_fresh_index", async () => {
    resetPlusFixture({ self: "enabled", catalog: { index: "never" } });
    renderPage();
    const index = await card();
    expect(await index.findByText("Not synced yet")).toBeInTheDocument();
    await userEvent.click(index.getByRole("button", { name: "Refresh now" }));
    expect(await index.findByText("Up to date")).toBeInTheDocument();
    expect(index.getByText("13,712 servers, synced 3 h ago")).toBeInTheDocument();
  });

  it("test24_a_refresh_that_fails_leaves_the_card_with_the_error", async () => {
    resetPlusFixture({ self: "enabled", catalog: { index: "stale", syncFails: true } });
    renderPage();
    const index = await card();
    await userEvent.click(await index.findByRole("button", { name: "Refresh now" }));
    expect(await index.findByText("Sync failed")).toBeInTheDocument();
    expect(index.getByRole("button", { name: "Refresh now" })).toBeEnabled();
  });
});
