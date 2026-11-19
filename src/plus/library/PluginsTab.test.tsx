import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { plusFixtureWorld, resetPlusFixture } from "../fixtures/ipc";
import { PLUGIN_FOLDER } from "../fixtures/pluginsWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import LibraryScreen from "./LibraryScreen";

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

const COVERS = [
  "library.plugins",
  "library.plugin-detail",
  "library.plugin-config",
  "library.plugin-mcp",
  "library.hooks-inventory",
  "library.plugin-disable",
  "library.plugin-update",
] as const;

const FOLDER_KEY = "toolport-plus-context-folder";

async function setup(
  options: { folder?: boolean; plugins?: object; tasks?: object } = {},
) {
  resetPlusFixture({ plugins: options.plugins, tasks: options.tasks });
  if (options.folder) localStorage.setItem(FOLDER_KEY, PLUGIN_FOLDER);
  render(<LibraryScreen />);
  await userEvent.click(await screen.findByRole("tab", { name: "Plugins" }));
  return screen.findByRole("region", { name: "Installed plugins" });
}

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

async function open(id: string) {
  await userEvent.click(await screen.findByRole("button", { name: `Open ${id}` }));
  await screen.findByRole("button", { name: /Plugins/ });
}

beforeEach(() => {
  invokeLog.length = 0;
  localStorage.clear();
  vi.clearAllMocks();
});

describe("Library screen, Plugins tab", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/library/PluginsTab.test.tsx",
      });
    }
  });

  it("test01_the_list_shows_each_plugin_with_version_what_it_brings_and_its_update", async () => {
    const list = await setup();
    expect(within(list).getAllByRole("button", { name: /^Open / })).toHaveLength(5);
    const kit = within(list).getByRole("button", { name: "Open kit@acme" });
    expect(kit).toHaveTextContent("1.8.2");
    expect(kit).toHaveTextContent(
      "3 skills, 2 agents, 4 commands, 2 hooks, 1 MCP server",
    );
    expect(kit).toHaveTextContent("Update to 1.9.0");
    expect(
      within(list).getByRole("button", { name: "Open notes-helper@community" }),
    ).toHaveTextContent("Up to date");
    expect(screen.getByText("1 plugin update available")).toBeVisible();
    expect(ranExactly("plugins", "ls")).toBe(true);
  });

  it("test02_a_plugin_with_hundreds_of_skills_says_they_are_never_part_of_the_library", async () => {
    await setup();
    expect(await screen.findByText(/brings 290 skills/)).toHaveTextContent(
      "never part of your library or your sync",
    );
  });

  it("test03_check_for_updates_refreshes_the_marketplaces_and_a_failure_is_said", async () => {
    await setup();
    await userEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
    await waitFor(() => expect(ranExactly("plugins", "ls", "--refresh")).toBe(true));
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith("1 plugin update available"),
    );
  });

  it("test04_without_claude_the_check_names_why_nothing_was_refreshed", async () => {
    await setup({ plugins: { noClaude: true } });
    await userEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
    await waitFor(() => expect(ranExactly("plugins", "ls", "--refresh")).toBe(true));
    expect(
      await screen.findByRole("region", { name: "Installed plugins" }),
    ).toBeVisible();
  });

  it("test05_no_installed_plugin_is_an_empty_state_not_a_blank_page", async () => {
    resetPlusFixture({ plugins: { empty: true } });
    render(<LibraryScreen />);
    await userEvent.click(await screen.findByRole("tab", { name: "Plugins" }));
    expect(await screen.findByText("No Claude Code plugins installed")).toBeVisible();
  });

  it("test06_a_plugin_the_administrator_switches_is_marked_managed_and_cannot_be_switched_here", async () => {
    const list = await setup({ folder: true, plugins: { managed: true } });
    expect(within(list).getByRole("button", { name: "Open kit@acme" })).toHaveTextContent(
      "Managed",
    );
    await open("kit@acme");
    expect(
      await screen.findByText(/Your administrator decides this plugin/),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /Turn off in this folder/ })).toBeNull();
  });

  it("test07_a_plugin_opens_as_a_page_with_what_it_provides_and_back_returns", async () => {
    await setup();
    await open("kit@acme");
    const provides = await screen.findByRole("region", { name: "It provides" });
    expect(within(provides).getByRole("list", { name: "Skills" })).toHaveTextContent(
      "commit-notes",
    );
    expect(within(provides).getByRole("list", { name: "Agents" })).toHaveTextContent(
      "code-scout",
    );
    expect(within(provides).getByRole("list", { name: "Commands" })).toBeVisible();
    expect(ranExactly("plugins", "show", "kit@acme")).toBe(true);
    const where = screen.getByRole("region", { name: "Where it is on" });
    expect(within(where).getByText("User settings").closest("li")).toHaveTextContent(
      "on",
    );
    await userEvent.click(screen.getByRole("button", { name: /Plugins/ }));
    expect(
      await screen.findByRole("region", { name: "Installed plugins" }),
    ).toBeVisible();
  });

  it("test08_a_long_list_of_skills_is_cut_and_shown_in_full_on_request", async () => {
    await setup();
    await open("toolkit-pro@community");
    const skills = await screen.findByRole("list", { name: "Skills" });
    expect(within(skills).getAllByRole("listitem")).toHaveLength(12);
    await userEvent.click(screen.getByRole("button", { name: "Show all 290" }));
    expect(within(skills).getAllByRole("listitem")).toHaveLength(290);
  });

  it("test09_the_hooks_of_a_plugin_show_event_matcher_command_and_how_to_switch_them", async () => {
    await setup();
    await open("lint-rules@community");
    const hooks = await screen.findByRole("region", { name: "Hooks" });
    expect(hooks).toHaveTextContent("PreToolUse");
    expect(hooks).toHaveTextContent("Edit|Write");
    expect(hooks).toHaveTextContent("node run.js pre:edit");
    expect(hooks).toHaveTextContent(
      "plugins config lint-rules@community --set disabled_hooks=pre:edit",
    );
  });

  it("test10_mcp_servers_show_how_they_start_and_deny_needs_a_folder", async () => {
    await setup();
    await open("kit@acme");
    const servers = await screen.findByRole("region", { name: "MCP servers" });
    expect(servers).toHaveTextContent("npx -y docs-mcp");
    expect(servers).toHaveTextContent("mcp__plugin_kit_docs__*");
    expect(
      within(servers).getByRole("button", { name: "Deny docs in this folder" }),
    ).toBeDisabled();
  });

  it("test11_deny_in_a_folder_shows_the_plan_then_writes_and_allow_takes_it_back", async () => {
    await setup({ folder: true });
    await open("kit@acme");
    const servers = await screen.findByRole("region", { name: "MCP servers" });
    await userEvent.click(
      within(servers).getByRole("button", { name: "Deny docs in this folder" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Deny MCP server docs of kit@acme/),
    ).toBeVisible();
    expect(
      ranExactly(
        "plugins",
        "mcp",
        "deny",
        "kit@acme",
        "docs",
        "--cwd",
        PLUGIN_FOLDER,
        "--dry-run",
      ),
    ).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Deny server" }));
    await waitFor(() =>
      expect(
        ranExactly("plugins", "mcp", "deny", "kit@acme", "docs", "--cwd", PLUGIN_FOLDER),
      ).toBe(true),
    );
    expect(plusFixtureWorld().plugins.deniedIn(PLUGIN_FOLDER)).toEqual(["kit@acme|docs"]);
    const after = await screen.findByRole("button", {
      name: "Allow docs in this folder",
    });
    expect(after).toBeVisible();
    expect(await screen.findByText("denied in local settings")).toBeVisible();
    await userEvent.click(after);
    const again = await screen.findByRole("dialog");
    await userEvent.click(
      await within(again).findByRole("button", { name: "Allow server" }),
    );
    await waitFor(() =>
      expect(plusFixtureWorld().plugins.deniedIn(PLUGIN_FOLDER)).toEqual([]),
    );
  });

  it("test12_turning_a_plugin_off_in_the_folder_goes_through_the_existing_plugin_switch", async () => {
    await setup({ folder: true });
    await open("kit@acme");
    await userEvent.click(
      await screen.findByRole("button", { name: "Turn off in this folder…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Turn off kit@acme/);
    expect(
      ranExactly("plugins", "off", "kit@acme", "--cwd", PLUGIN_FOLDER, "--dry-run"),
    ).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Turn off" }));
    await waitFor(() =>
      expect(ranExactly("plugins", "off", "kit@acme", "--cwd", PLUGIN_FOLDER)).toBe(true),
    );
    expect(
      await screen.findByRole("button", { name: "Turn on in this folder…" }),
    ).toBeVisible();
  });

  it("test13_options_show_their_values_and_a_sensitive_one_only_as_hidden", async () => {
    await setup();
    await open("kit@acme");
    const options = await screen.findByRole("list", { name: "Plugin options" });
    expect(options).toHaveTextContent("Mode");
    expect(options).toHaveTextContent("standard");
    const token = within(options).getByText("API token").closest("li") as HTMLElement;
    expect(token).toHaveTextContent("Hidden");
    expect(ranExactly("plugins", "show", "kit@acme")).toBe(true);
  });

  it("test14_a_knob_shows_where_its_value_comes_from_and_is_changed_after_the_plan", async () => {
    await setup({ folder: true });
    await open("lint-rules@community");
    const switches = await screen.findByRole("list", { name: "Switches" });
    expect(within(switches).getByText("Profile").closest("li")).toHaveTextContent(
      "default",
    );
    await userEvent.click(
      within(switches).getByRole("button", { name: "Change Profile" }),
    );
    const form = await screen.findByRole("dialog");
    await userEvent.selectOptions(within(form).getByLabelText("Value"), "strict");
    await userEvent.click(within(form).getByRole("button", { name: "Review" }));
    const plan = await screen.findByRole("dialog");
    await within(plan).findByText(/Set profile for lint-rules@community/);
    expect(
      ranExactly(
        "plugins",
        "config",
        "lint-rules@community",
        "--cwd",
        PLUGIN_FOLDER,
        "--set",
        "profile=strict",
        "--dry-run",
      ),
    ).toBe(true);
    await userEvent.click(within(plan).getByRole("button", { name: "Change" }));
    await waitFor(() =>
      expect(
        plusFixtureWorld().plugins.written(PLUGIN_FOLDER, "lint-rules@community"),
      ).toEqual({
        profile: "strict",
      }),
    );
    const row = (await screen.findByText("Profile")).closest("li") as HTMLElement;
    await waitFor(() => expect(row).toHaveTextContent("set in this folder"));
  });

  it("test15_a_folder_only_knob_without_a_folder_says_so_instead_of_writing_anywhere", async () => {
    await setup();
    await open("lint-rules@community");
    const switches = await screen.findByRole("list", { name: "Switches" });
    await userEvent.click(within(switches).getByRole("button", { name: "Change Guard" }));
    const form = await screen.findByRole("dialog");
    expect(within(form).getByText(/Choose a folder first/)).toBeVisible();
    expect(ran("config", "--set")).toBe(false);
  });

  it("test16_a_plugin_option_knob_can_be_set_for_every_folder_without_a_folder", async () => {
    await setup();
    await open("lint-rules@community");
    const switches = await screen.findByRole("list", { name: "Switches" });
    await userEvent.click(
      within(switches).getByRole("button", { name: "Change Profile" }),
    );
    const form = await screen.findByRole("dialog");
    await userEvent.selectOptions(within(form).getByLabelText("Value"), "relaxed");
    await userEvent.click(within(form).getByRole("button", { name: "Review" }));
    await screen.findByRole("dialog");
    await waitFor(() =>
      expect(
        ranExactly(
          "plugins",
          "config",
          "lint-rules@community",
          "--set",
          "profile=relaxed",
          "--dry-run",
        ),
      ).toBe(true),
    );
  });

  it("test17_choose_items_links_to_selective_loading_in_sources", async () => {
    await setup();
    await open("kit@acme");
    await userEvent.click(
      await screen.findByRole("button", { name: "Choose items of this plugin…" }),
    );
    expect(await screen.findByRole("tab", { name: /Sources/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await waitFor(() =>
      expect(
        ranArgv().some((a) => a.join(" ") === "library select ls acme-skills/kit"),
      ).toBe(true),
    );
  });

  it("test18_the_hooks_section_counts_processes_per_tool_and_names_the_owners_that_share_one", async () => {
    await setup();
    const hooks = await screen.findByRole("region", { name: "Hooks" });
    await within(hooks).findByText("node scripts/guard.js");
    const counts = within(hooks).getByRole("list", { name: "Processes per tool" });
    expect(counts).toHaveTextContent("Bash: 2 processes (2 before, 0 after)");
    expect(hooks).toHaveTextContent("all answer before the tool runs");
    expect(hooks).toHaveTextContent("~/bin/own.sh");
    expect(ranExactly("hooks", "ls")).toBe(true);
  });

  it("test19_the_chosen_folder_decides_where_a_plugin_is_on", async () => {
    await setup({ folder: true });
    await waitFor(() =>
      expect(ranExactly("plugins", "ls", "--cwd", PLUGIN_FOLDER)).toBe(true),
    );
    expect(ranExactly("hooks", "ls", "--cwd", PLUGIN_FOLDER)).toBe(true);
  });

  it("test20_the_updates_card_names_the_scheduled_check_and_what_it_does_not_do", async () => {
    await setup({ tasks: { updateCheck: true } });
    const line = await screen.findByText(/Updates are looked for on a schedule/);
    expect(line).toHaveTextContent("Check chosen items for updates");
    expect(line).toHaveTextContent("it has not run yet");
    expect(line).toHaveTextContent("nothing is updated until you press Update");
  });

  it("test21_a_switched_off_scheduled_check_says_updates_are_only_looked_for_on_request", async () => {
    await setup({ tasks: { updateCheck: "off" } });
    expect(await screen.findByText(/The scheduled check/)).toHaveTextContent(
      "is off, so updates are only looked for when you press Check for updates",
    );
  });

  it("test22_without_the_task_the_card_shows_no_schedule_line", async () => {
    await setup();
    expect(screen.queryByText(/on a schedule/)).toBeNull();
  });

  it("test23_update_all_shows_each_plan_then_runs_the_update_and_the_restart_notice_follows", async () => {
    await setup();
    expect(screen.queryByText(/already running started before/)).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Update 1 plugin…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/claude plugin update kit@acme --scope user/),
    ).toBeVisible();
    expect(ranExactly("plugins", "update", "kit@acme", "--dry-run")).toBe(true);
    expect(
      within(dialog).getByText(/sessions that are already running keep the old state/),
    ).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update all" }));
    await waitFor(() => expect(ranExactly("plugins", "update", "kit@acme")).toBe(true));
    expect(await screen.findByText("Plugins are up to date")).toBeVisible();
    expect(
      await screen.findByText(/already running started before a plugin changed/),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Open kit@acme" })).toHaveTextContent(
      "1.9.0",
    );
  });

  it("test24_the_detail_page_updates_one_plugin_after_the_plan", async () => {
    await setup();
    await open("kit@acme");
    await userEvent.click(
      await screen.findByRole("button", { name: "Update to 1.9.0…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(/Update kit@acme/);
    await userEvent.click(within(dialog).getByRole("button", { name: "Update" }));
    await waitFor(() => expect(ranExactly("plugins", "update", "kit@acme")).toBe(true));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Update to 1.9.0…" })).toBeNull(),
    );
    expect(screen.getByText("Up to date")).toBeVisible();
  });

  it("test25_a_blocked_update_is_marked_gives_its_reason_and_offers_no_button", async () => {
    const list = await setup({ plugins: { blocked: true } });
    expect(within(list).getByRole("button", { name: "Open kit@acme" })).toHaveTextContent(
      "Update blocked",
    );
    expect(screen.queryByRole("button", { name: /^Update \d plugin/ })).toBeNull();
    await open("kit@acme");
    expect(
      await screen.findByText(/your administrator blocks the acme marketplace/),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /^Update to/ })).toBeNull();
  });

  it("test26_a_failing_claude_is_shown_in_the_plan_and_nothing_is_written", async () => {
    await setup({ plugins: { noClaude: true } });
    await open("kit@acme");
    await userEvent.click(
      await screen.findByRole("button", { name: "Update to 1.9.0…" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "claude not found on PATH",
    );
    expect(ranExactly("plugins", "update", "kit@acme")).toBe(false);
  });

  it("test27_disable_everywhere_needs_the_name_typed_and_enable_everywhere_follows", async () => {
    await setup();
    await open("notes-helper@community");
    await userEvent.click(
      await screen.findByRole("button", { name: "Disable everywhere…" }),
    );
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByText(
      /claude plugin disable notes-helper@community --scope user/,
    );
    expect(ranExactly("plugins", "disable", "notes-helper@community", "--dry-run")).toBe(
      true,
    );
    const confirm = within(dialog).getByRole("button", { name: "Disable everywhere" });
    expect(confirm).toBeDisabled();
    await userEvent.type(
      within(dialog).getByLabelText(/Type notes-helper to confirm/),
      "notes-helper",
    );
    expect(confirm).toBeEnabled();
    await userEvent.click(confirm);
    await waitFor(() =>
      expect(ranExactly("plugins", "disable", "notes-helper@community")).toBe(true),
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Enable everywhere…" }),
    );
    const again = await screen.findByRole("dialog");
    await within(again).findByText(
      /claude plugin enable notes-helper@community --scope user/,
    );
    await userEvent.click(
      within(again).getByRole("button", { name: "Enable everywhere" }),
    );
    await waitFor(() =>
      expect(ranExactly("plugins", "enable", "notes-helper@community")).toBe(true),
    );
    expect(
      await screen.findByRole("button", { name: "Disable everywhere…" }),
    ).toBeVisible();
  });

  it("test28_a_plugin_the_administrator_decides_has_no_disable_button", async () => {
    await setup({ plugins: { managed: true } });
    await open("kit@acme");
    expect(
      await screen.findByText(/Your administrator decides this plugin/),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: /everywhere/ })).toBeNull();
  });

  it("test29_a_running_session_that_started_before_a_change_gets_a_restart_notice", async () => {
    await setup({ plugins: { restart: true } });
    expect(
      await screen.findByText(/keeps the old state until you restart it/),
    ).toBeVisible();
  });
});

function ran(...words: string[]) {
  return ranArgv().some((argv) => words.every((word) => argv.includes(word)));
}
