import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { plusFixtureIpc, resetPlusFixture } from "../fixtures/ipc";
import type { SelectScenario } from "../fixtures/selectWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import SelectScreen from "./SelectScreen";
import SourcesTab from "./SourcesTab";

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

const SOURCE = "acme-skills";

function setup(select: SelectScenario) {
  resetPlusFixture({ library: { select } });
  const onBack = vi.fn();
  render(<SelectScreen source={SOURCE} onBack={onBack} />);
  return onBack;
}

const ran = (...prefix: string[]) =>
  ranArgv().filter((argv) => prefix.every((word, i) => argv[i] === word));
const ranSelect = (command: string) => ran("library", "select", command);

async function ready() {
  await screen.findByRole("region", { name: "Skills" });
}

const box = (name: RegExp | string) => screen.getByRole("checkbox", { name });

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library > Sources > Choose items", () => {
  it("test00_the_select_parity_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.select"]).toMatchObject({
      status: "built",
      test: "src/plus/library/SelectScreen.test.tsx",
    });
  });

  it("test01_items_are_grouped_by_kind_with_counts_and_new_ones_start_off", async () => {
    setup("chosen");
    await ready();
    expect(screen.getByRole("heading", { name: "Skills (4)" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Hooks (2)" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Rules (1)" })).toBeVisible();
    expect(document.body).toHaveTextContent("3 of 10 items on");
    expect(box("skill code-review")).toBeChecked();
    expect(box("skill deploy-runbook")).not.toBeChecked();
    expect(box("skill deploy-runbook").closest("li")).toHaveTextContent("off");
  });

  it("test02_a_source_nobody_chose_from_says_so_and_loads_nothing", async () => {
    setup("fresh");
    await ready();
    expect(screen.getByText(/Nothing is chosen yet/)).toBeVisible();
    expect(box("skill code-review").closest("li")).toHaveTextContent("new");
    expect(screen.getByRole("button", { name: "Review plan" })).toBeDisabled();
  });

  it("test03_search_and_the_new_filter_narrow_the_list", async () => {
    setup("fresh");
    await ready();
    await userEvent.type(screen.getByRole("textbox", { name: "Search items" }), "deploy");
    expect(screen.getByRole("checkbox", { name: "skill deploy-runbook" })).toBeVisible();
    expect(screen.queryByRole("checkbox", { name: "skill code-review" })).toBeNull();
    await userEvent.clear(screen.getByRole("textbox", { name: "Search items" }));
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Kind" }),
      "agent",
    );
    expect(screen.queryByRole("region", { name: "Skills" })).toBeNull();
    expect(screen.getByRole("region", { name: "Agents" })).toBeVisible();
  });

  it("test04_a_plan_is_shown_before_anything_is_applied_and_apply_needs_its_hash", async () => {
    setup("fresh");
    await ready();
    await userEvent.click(box("skill code-review"));
    await userEvent.click(box("agent reviewer"));
    await userEvent.click(screen.getByRole("button", { name: "Apply 2 changes" }));
    await screen.findByRole("region", { name: "Update plan" });
    expect(ranSelect("set")).toEqual([
      [
        "library",
        "select",
        "set",
        SOURCE,
        "--on",
        "agent:reviewer",
        "--on",
        "skill:code-review",
      ],
    ]);
    expect(ranSelect("plan")).toHaveLength(1);
    expect(ranSelect("apply")).toHaveLength(0);
    expect(screen.getByText(/Nothing changes until you apply this plan/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Apply plan" }));
    await screen.findByRole("region", { name: "Applied" });
    const [apply] = ranSelect("apply");
    expect(apply?.[4]).toBe("--plan-hash");
    expect(apply?.[5]).toMatch(/^sha256:/);
  });

  it("test05_a_hook_needs_its_own_tick_and_is_never_turned_on_with_on", async () => {
    setup("fresh");
    await ready();
    await userEvent.click(box(/hook PreToolUse Bash/));
    const dialog = await screen.findByRole("dialog", { name: "Review hook" });
    expect(within(dialog).getByText("echo checked")).toBeVisible();
    const turnOn = within(dialog).getByRole("button", { name: "Turn on hook" });
    expect(turnOn).toBeDisabled();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(box(/hook PreToolUse Bash/)).not.toBeChecked();
    await userEvent.click(box(/hook PreToolUse Bash/));
    await userEvent.click(
      screen.getByRole("checkbox", { name: /I have read this hook/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Turn on hook" }));
    expect(box(/hook PreToolUse Bash/)).toBeChecked();
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    await screen.findByRole("region", { name: "Update plan" });
    const [set] = ranSelect("set");
    expect(set).toContain("--tick-hook");
    expect(set).not.toContain("--on");
  });

  it("test06_the_footer_shows_the_command_line_for_what_is_staged", async () => {
    setup("chosen");
    await ready();
    await userEvent.click(box("skill deploy-runbook"));
    const footer = screen.getByRole("region", { name: "From a terminal" });
    expect(footer).toHaveTextContent(
      `toolportctl library select set ${SOURCE} --on skill:deploy-runbook`,
    );
    expect(footer).toHaveTextContent(`toolportctl library select plan ${SOURCE}`);
    expect(footer).toHaveTextContent("--plan-hash <hash>");
  });

  it("test07_an_update_lists_changes_hooks_new_and_removed_items_and_applies_nothing", async () => {
    setup("update");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    const plan = await screen.findByRole("region", { name: "Update plan" });
    expect(plan).toHaveTextContent("a1b2c3d → e4f5a6b");
    const changed = within(plan).getByRole("region", {
      name: /Your items that change \(1\)/,
    });
    expect(changed).toHaveTextContent("content changed (+12 / −3 lines)");
    const hooks = within(plan).getByRole("region", { name: /Hooks that changed \(1\)/ });
    expect(hooks).toHaveTextContent("needs your tick again");
    expect(hooks).toHaveTextContent("Stays on the old version until ticked");
    expect(hooks).toHaveTextContent("--all");
    const fresh = within(plan).getByRole("region", { name: /New in the source \(2\)/ });
    expect(fresh).toHaveTextContent("off by default");
    expect(
      within(plan).getByRole("region", { name: /Removed upstream \(1\)/ }),
    ).toHaveTextContent("keeps working from the pinned copy");
    expect(ranSelect("apply")).toHaveLength(0);
  });

  it("test08_applying_an_update_leaves_new_items_off_and_asks_for_the_hook_tick_again", async () => {
    setup("update");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await screen.findByRole("region", { name: "Update plan" });
    await userEvent.click(screen.getByRole("button", { name: "Apply plan" }));
    await userEvent.click(
      await screen.findByRole("button", { name: "Back to the items" }),
    );
    await ready();
    expect(box("skill perf-profile")).not.toBeChecked();
    expect(box("skill perf-profile").closest("li")).toHaveTextContent("new");
    const format = box(/hook PostToolUse Write\|Edit/).closest("li");
    expect(format).toHaveTextContent("tick");
    expect(format).toHaveTextContent("command changed upstream");
    await userEvent.click(box(/hook PostToolUse Write\|Edit/));
    const dialog = await screen.findByRole("dialog", { name: "Review hook" });
    expect(within(dialog).getByText(/--all/, { selector: "dd" })).toBeVisible();
    await userEvent.click(within(dialog).getByRole("checkbox"));
    await userEvent.click(within(dialog).getByRole("button", { name: "Turn on hook" }));
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    await screen.findByRole("region", { name: "Update plan" });
    expect(ranSelect("set").at(-1)).toContain("--tick-hook");
  });

  it("test09_a_hook_that_changed_can_be_read_in_the_plan_but_not_ticked_there", async () => {
    setup("update");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    const plan = await screen.findByRole("region", { name: "Update plan" });
    await userEvent.click(
      within(plan).getAllByRole("button", { name: "Review hook" })[0]!,
    );
    const dialog = await screen.findByRole("dialog", { name: "Review hook" });
    expect(within(dialog).queryByRole("button", { name: "Turn on hook" })).toBeNull();
    expect(dialog).toHaveTextContent("The old command keeps running");
  });

  it("test10_an_item_removed_upstream_keeps_its_pinned_copy_until_removed", async () => {
    setup("gone-only");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    const gone = await screen.findByRole("region", { name: /Removed upstream \(1\)/ });
    await userEvent.click(within(gone).getByRole("button", { name: "Keep pinned copy" }));
    expect(gone).toHaveTextContent("keeps working from the pinned copy");
    expect(ranSelect("set")).toHaveLength(0);
    await userEvent.click(within(gone).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(ranSelect("set")).toHaveLength(1));
    expect(ranSelect("set")[0]).toEqual([
      "library",
      "select",
      "set",
      SOURCE,
      "--off",
      "skill:legacy-lint",
    ]);
  });

  it("test11_undo_runs_library_select_undo_and_restores_the_list", async () => {
    setup("chosen");
    await ready();
    await userEvent.click(box("skill deploy-runbook"));
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    await userEvent.click(await screen.findByRole("button", { name: "Apply plan" }));
    await screen.findByRole("region", { name: "Applied" });
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(ranSelect("undo")).toHaveLength(1));
    expect(ranSelect("undo")).toEqual([["library", "select", "undo", SOURCE]]);
    await ready();
    expect(await screen.findByRole("button", { name: "Review plan" })).toBeEnabled();
  });

  it("test12_a_header_undo_with_nothing_to_undo_says_so_and_changes_nothing", async () => {
    setup("chosen");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: /Undo last apply/ }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(ranSelect("undo")).toHaveLength(1));
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("test13_a_plan_that_went_stale_is_refused_and_can_be_made_again", async () => {
    setup("fresh");
    await ready();
    await userEvent.click(box("skill code-review"));
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    await screen.findByRole("region", { name: "Update plan" });
    const job = plusFixtureIpc("plus_ctl", {
      argv: ["library", "select", "set", SOURCE, "--on", "skill:tdd-workflow"],
    });
    plusFixtureIpc("plus_ctl_result", { job });
    await userEvent.click(screen.getByRole("button", { name: "Apply plan" }));
    expect(await screen.findByText(/the plan changed since it was made/)).toBeVisible();
    expect(screen.queryByRole("region", { name: "Applied" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Plan again" }));
    await userEvent.click(await screen.findByRole("button", { name: "Apply plan" }));
    await screen.findByRole("region", { name: "Applied" });
  });

  it("test14_a_source_that_cannot_be_checked_keeps_the_selection_and_offers_retry", async () => {
    setup("offline");
    await ready();
    await userEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    const banner = await screen.findByText(/Could not check for a newer version/);
    expect(banner).toHaveTextContent("last good a1b2c3d");
    expect(banner).toHaveTextContent("could not resolve host");
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(ranSelect("plan")).toHaveLength(2));
  });

  it("test15_hooks_switched_off_by_the_claude_setting_are_marked_on_the_ticked_hook", async () => {
    setup("hooks-off");
    await ready();
    expect(screen.getByText(/Hooks are off by your Claude Code settings/)).toBeVisible();
    expect(box(/hook PostToolUse Write\|Edit/).closest("li")).toHaveTextContent(
      "off by setting",
    );
  });

  it("test16_a_source_blocked_by_policy_cannot_be_chosen_from", async () => {
    setup("blocked");
    await ready();
    expect(screen.getByText(/Blocked by policy/)).toBeVisible();
    expect(box("skill code-review")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Select none" })).toBeDisabled();
  });

  it("test17_a_plugin_installed_whole_offers_the_switch_and_only_asks_for_it", async () => {
    setup("whole");
    await ready();
    const offer = screen.getByRole("checkbox", { name: /Installed whole/ });
    expect(offer).not.toBeChecked();
    await userEvent.click(box("skill code-review"));
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    await screen.findByRole("region", { name: "Update plan" });
    expect(ranSelect("plan")[0]).not.toContain("--switch");
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    await ready();
    await userEvent.click(screen.getByRole("checkbox", { name: /Installed whole/ }));
    await userEvent.click(screen.getByRole("button", { name: "Review plan" }));
    await screen.findByRole("region", { name: "Update plan" });
    expect(ranSelect("plan").at(-1)).toContain("--switch");
    expect(screen.getByText(/disable acme-skills@acme-skills/)).toBeInTheDocument();
  });

  it("test18_everything_on_is_allowed_but_shown_as_a_visible_decision", async () => {
    setup("everything");
    await ready();
    expect(screen.getByText(/Everything is on: all 10 items will load/)).toBeVisible();
  });

  it("test19_rules_are_listed_as_linked_through_the_library_not_the_plugin", async () => {
    setup("chosen");
    await ready();
    await userEvent.click(box("rule style"));
    await userEvent.click(screen.getByRole("button", { name: "Apply 1 change" }));
    const plan = await screen.findByRole("region", { name: "Update plan" });
    expect(plan).toHaveTextContent("Link them through the Library");
  });

  it("test20_rows_wrap_on_a_phone_width_so_actions_stay_inside_the_row", async () => {
    setup("chosen");
    await ready();
    const row = box("skill code-review").closest("li");
    expect(row).toHaveClass("flex-wrap");
    expect(row?.querySelector("p")).toHaveClass("break-words");
    expect(
      screen.getByRole("region", { name: "From a terminal" }).querySelector("pre"),
    ).toHaveClass("whitespace-pre-wrap");
  });

  it("test21_an_unknown_source_shows_the_error_and_a_way_back", async () => {
    resetPlusFixture({ library: { select: "fresh" } });
    const onBack = vi.fn();
    render(<SelectScreen source="nope" onBack={onBack} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Tap 'nope' not found");
    await userEvent.click(screen.getByRole("button", { name: /Sources/ }));
    expect(onBack).toHaveBeenCalled();
  });

  it("test22_a_tap_row_opens_its_items_from_the_sources_tab", async () => {
    resetPlusFixture({ library: { select: "chosen" } });
    render(<SourcesTab />);
    await userEvent.click(
      await screen.findByRole("button", { name: `Choose items of ${SOURCE}` }),
    );
    await screen.findByRole("region", { name: "Choose items" });
    expect(await screen.findByRole("region", { name: "Skills" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: /Sources/ }));
    expect(await screen.findByRole("region", { name: "Taps" })).toBeVisible();
  });
});
