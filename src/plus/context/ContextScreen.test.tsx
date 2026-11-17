import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { FIXTURE_FOLDER, type ContextWorldOptions } from "../fixtures/contextWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import ContextScreen from "./ContextScreen";

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
  "context.folder",
  "context.status",
  "context.leave-out",
  "context.plugin-switch",
  "context.measure",
  "context.bundles",
  "context.bundle-apply",
  "context.use",
  "context.sync",
] as const;

const ORG = "/Library/Application Support/ClaudeCode/CLAUDE.md";

function setup(
  context: ContextWorldOptions = {},
  folder: string | null = FIXTURE_FOLDER,
) {
  resetPlusFixture({ context });
  localStorage.clear();
  if (folder) localStorage.setItem("toolport-plus-context-folder", folder);
  return render(<ContextScreen />);
}

async function onThisFolder() {
  await screen.findByTestId("headline-total");
}

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Context screen", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/context/ContextScreen.test.tsx",
      });
    }
    expect(guiParity.routes.context).toMatchObject({ status: "built" });
  });

  it("test01_context_folder_asks_for_a_folder_first_and_shows_the_estimate_for_a_typed_one", async () => {
    setup({}, null);
    expect(await screen.findByRole("status")).toHaveTextContent("Choose a folder");
    await userEvent.click(screen.getByRole("button", { name: "Choose a folder" }));
    await userEvent.type(screen.getByLabelText("Folder path"), FIXTURE_FOLDER);
    await userEvent.click(screen.getByRole("button", { name: "Use" }));
    expect(await screen.findByTestId("headline-total")).toHaveTextContent(
      "≈ 19,420 tokens",
    );
    expect(screen.getByText("Estimate")).toBeVisible();
    expect(localStorage.getItem("toolport-plus-context-folder")).toBe(FIXTURE_FOLDER);
    expect(ran("context", "loads", "--measured", FIXTURE_FOLDER)).toBe(true);
    expect(ran("context", "status", "--cwd", FIXTURE_FOLDER)).toBe(true);
  });

  it("test02_context_folder_lists_the_stack_with_layers_the_biggest_levers_and_not_itemised", async () => {
    setup();
    await onThisFolder();
    const levers = screen.getByRole("region", { name: "Biggest levers here" });
    expect(
      within(levers).getByRole("button", { name: "Leave out CLAUDE.md (10,070 tokens)" }),
    ).toBeVisible();
    expect(within(levers).getByText("−10,070")).toBeVisible();
    const stack = screen.getByRole("region", { name: "Instruction files" });
    expect(within(stack).getAllByRole("switch")).toHaveLength(6);
    expect(stack).toHaveTextContent(/Org\s*10,070/);
    expect(screen.getByRole("region", { name: "Loads on demand" })).toHaveTextContent(
      "2,020",
    );
    expect(screen.getByText(/skills, plugins, agents, tool definitions/)).toBeVisible();
  });

  it("test03_context_leave_out_is_one_click_with_an_undo_that_puts_the_file_back", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(
      screen.getByRole("switch", { name: "CLAUDE.md (Org) loads here" }),
    );
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "leave-out",
        "add",
        ORG,
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "CLAUDE.md (Org) loads here" }),
      ).not.toBeChecked(),
    );
    expect(screen.getByTestId("headline-total")).toHaveTextContent("≈ 9,350 tokens");
    const [message, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
    expect(message).toBe("CLAUDE.md is left out of this folder");
    expect(options).toMatchObject({ duration: 10000, action: { label: "Undo" } });
    (options as unknown as { action: { onClick: () => void } }).action.onClick();
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "leave-out",
        "rm",
        ORG,
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "CLAUDE.md (Org) loads here" }),
      ).toBeChecked(),
    );
  });

  it("test04_context_leave_out_memory_switch_turns_auto_memory_off_for_this_folder", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("switch", { name: "Auto memory loads here" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "leave-out",
        "add",
        "--memory",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    const ledger = await screen.findByRole("region", {
      name: "What Toolport changed here",
    });
    expect(within(ledger).getByText("Auto memory off")).toBeVisible();
  });

  it("test05_context_plugin_switch_turns_a_plugin_off_here_and_back_on", async () => {
    setup();
    await onThisFolder();
    await userEvent.type(screen.getByLabelText("Plugin id"), "suite-a@market");
    await userEvent.click(screen.getByRole("button", { name: "Turn off here" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "plugins",
        "off",
        "suite-a@market",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    const sw = await screen.findByRole("switch", { name: "suite-a@market loads here" });
    await waitFor(() => expect(sw).not.toBeChecked());
    expect(screen.getByText("Off here")).toBeVisible();
    await userEvent.click(sw);
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "plugins",
        "on",
        "suite-a@market",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
  });

  it("test06_context_status_shows_measured_with_the_not_itemised_part_and_projects_after_a_change", async () => {
    setup({ measured: "fresh" });
    await onThisFolder();
    expect(screen.getByTestId("headline-total")).toHaveTextContent("37,470 tokens");
    expect(screen.getByText("Measured")).toBeVisible();
    expect(screen.getByRole("img", { name: /not itemised/ })).toHaveAccessibleName(
      "19,420 tokens itemised, 18,050 not itemised",
    );
    await userEvent.click(
      screen.getByRole("switch", { name: "CLAUDE.md (Org) loads here" }),
    );
    await waitFor(() => expect(screen.getByText("Projected")).toBeVisible());
    expect(screen.getByTestId("headline-total")).toHaveTextContent("27,400 tokens");
    expect(screen.getByText("Applies from the next session")).toBeVisible();
  });

  it("test07_context_status_keeps_a_stale_measurement_with_a_word", async () => {
    setup({ measured: "stale" });
    await onThisFolder();
    expect(screen.getByText("Measured, stale")).toBeVisible();
    expect(screen.getByTestId("headline-total")).toHaveTextContent("37,470 tokens");
  });

  it("test08_context_measure_confirms_with_the_plan_runs_and_shows_the_result", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("button", { name: "Measure for real…" }));
    const plan = await screen.findByRole("region", { name: "Measure plan" });
    expect(within(plan).getByText("1 request on haiku")).toBeVisible();
    expect(ran("context", "measure", "--yes")).toBe(false);
    await userEvent.click(within(plan).getByRole("button", { name: "Start 1 request" }));
    const result = await screen.findByRole("region", { name: "Measured result" });
    expect(ran("context", "measure", "--cwd", FIXTURE_FOLDER, "--yes")).toBe(true);
    expect(within(result).getByText("37,470 tokens")).toBeVisible();
    expect(within(result).getByText(/Not itemised 18,050/)).toBeVisible();
    expect(screen.getByTestId("verified")).toHaveTextContent("7 loaded");
    await userEvent.click(screen.getByRole("button", { name: "Back to this folder" }));
    expect(await screen.findByText("Measured")).toBeVisible();
  });

  it("test09_context_measure_without_this_asks_for_one_more_request_per_change", async () => {
    setup({ measured: "fresh" });
    await onThisFolder();
    await userEvent.click(screen.getByRole("button", { name: "Measure for real…" }));
    await screen.findByRole("region", { name: "Measured result" });
    await userEvent.click(screen.getByRole("button", { name: "Without suite-a@market" }));
    const plan = await screen.findByRole("region", { name: "Measure plan" });
    expect(within(plan).getByText("1 request on haiku")).toBeVisible();
    expect(
      ranArgv().some((a) => a.join(" ").includes("--without plugin:suite-a@market")),
    ).toBe(true);
    await userEvent.click(within(plan).getByRole("button", { name: "Start 1 request" }));
    expect(await screen.findByText("-7,450 (-20%)")).toBeVisible();
  });

  it("test10_context_measure_failure_keeps_the_estimate_and_says_why", async () => {
    setup({ measure: "fail" });
    await onThisFolder();
    await userEvent.click(screen.getByRole("button", { name: "Measure for real…" }));
    await userEvent.click(await screen.findByRole("button", { name: "Start 1 request" }));
    await waitFor(() => expect(vi.mocked(toast.error)).toHaveBeenCalled());
    expect(vi.mocked(toast.error).mock.calls[0][0]).toBe("The measurement failed");
    await userEvent.click(screen.getByRole("button", { name: "Back to this folder" }));
    expect(await screen.findByText("Estimate")).toBeVisible();
  });

  it("test11_context_bundles_lists_the_bundles_and_applies_one_after_its_plan", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Folder bundles" }));
    const detail = await screen.findByRole("region", { name: "Bundle Stage" });
    expect(
      screen.getAllByRole("button", { name: /^(Stage|default|erp-dev)/ }),
    ).toHaveLength(3);
    await userEvent.click(within(detail).getByRole("button", { name: /^Apply to / }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/claudeMdExcludes/)).toBeVisible();
    expect(ranArgv()).toContainEqual([
      "context",
      "bundle",
      "apply",
      "Stage",
      "--cwd",
      FIXTURE_FOLDER,
      "--dry-run",
    ]);
    expect(
      ran("bundle", "apply", "Stage") &&
        ranArgv().filter((a) => a[2] === "apply" && !a.includes("--dry-run")),
    ).toHaveLength(0);
    await userEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "bundle",
        "apply",
        "Stage",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    expect(await screen.findAllByText("applied in 1 folder")).not.toHaveLength(0);
  });

  it("test12_context_bundle_apply_undo_shows_the_plan_and_puts_the_keys_back", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Folder bundles" }));
    const detail = await screen.findByRole("region", { name: "Bundle Stage" });
    await userEvent.click(within(detail).getByRole("button", { name: /^Apply to / }));
    await userEvent.click(await screen.findByRole("button", { name: "Apply" }));
    await userEvent.click(await within(detail).findByRole("button", { name: "Undo…" }));
    await userEvent.click(await screen.findByRole("button", { name: "Undo" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "bundle",
        "undo",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    expect(await within(detail).findByText("Not applied in any folder")).toBeVisible();
  });

  it("test13_context_use_applies_from_this_folder_and_stops_using_it_again", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("button", { name: "Apply a bundle…" }));
    await userEvent.click(await screen.findByRole("button", { name: /^erp-dev/ }));
    await userEvent.click(await screen.findByRole("button", { name: "Use bundle" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "use",
        "erp-dev",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    expect(await screen.findByText("erp-dev")).toBeVisible();
    await userEvent.click(await screen.findByRole("button", { name: "Stop using…" }));
    await userEvent.click(await screen.findByRole("button", { name: "Stop using" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "use",
        "--none",
        "--cwd",
        FIXTURE_FOLDER,
      ]),
    );
    expect(screen.queryByRole("button", { name: "Folder bundles…" })).toBeNull();
  });

  it("test14_context_bundles_creates_edits_and_deletes_a_bundle_after_its_plan", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Folder bundles" }));
    await screen.findByRole("region", { name: "Bundle Stage" });
    await userEvent.click(
      screen.getByRole("button", { name: "Create from this folder…" }),
    );
    await userEvent.type(screen.getByLabelText("Name"), "lean");
    await userEvent.type(screen.getByLabelText("Description"), "Lean");
    await userEvent.click(screen.getByRole("button", { name: "Review the plan" }));
    await userEvent.click(await screen.findByRole("button", { name: "Save bundle" }));
    await waitFor(() =>
      expect(
        ranArgv().some(
          (a) =>
            a.slice(0, 4).join(" ") === "context bundle add lean" &&
            a.includes("--from-folder") &&
            !a.includes("--dry-run"),
        ),
      ).toBe(true),
    );
    expect(await screen.findByRole("region", { name: "Bundle lean" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete…" }));
    await userEvent.click(await screen.findByRole("button", { name: "Delete bundle" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["context", "bundle", "rm", "lean"]),
    );
  });

  it("test15_context_bundle_apply_switch_sets_auto_apply_and_sync_reads_it", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Folder bundles" }));
    const auto = await screen.findByRole("switch", {
      name: "Apply automatically in bound folders",
    });
    await userEvent.click(auto);
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "context",
        "bundle",
        "config",
        "--auto-apply",
        "on",
      ]),
    );
    await userEvent.click(screen.getByRole("tab", { name: "Rules & launch" }));
    expect(await screen.findByText("auto-apply on")).toBeVisible();
    expect(screen.getByText(/1 folder would get a bundle/)).toBeVisible();
    expect(screen.getAllByText(/not part of this build yet/).length).toBeGreaterThan(0);
  });

  it("test16_context_sync_runs_context_sync_when_sync_now_is_pressed", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Rules & launch" }));
    await userEvent.click(await screen.findByRole("button", { name: "Sync now" }));
    await waitFor(() => expect(ranArgv()).toContainEqual(["context", "sync"]));
    expect(ranArgv()).toContainEqual(["context", "sync", "--dry-run"]);
  });

  it("test17_hooks_and_compression_reads_the_hook_state_of_the_machine", async () => {
    setup();
    await onThisFolder();
    await userEvent.click(screen.getByRole("tab", { name: "Hooks & compression" }));
    expect(
      await screen.findByText("Toolport found 2 compression hooks it did not write"),
    ).toBeVisible();
    expect(ranArgv()).toContainEqual(["compression", "hooks", "status"]);
  });

  it("test18_tabs_follow_the_arrow_keys_and_the_folder_chip_belongs_to_three_of_them", async () => {
    setup();
    await onThisFolder();
    const first = screen.getByRole("tab", { name: "This folder" });
    first.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Folder bundles" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "Hooks & compression" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByRole("button", { name: /^Folder: / })).toBeVisible();
    await userEvent.click(screen.getByRole("tab", { name: "Rules & launch" }));
    expect(screen.queryByRole("button", { name: /^Folder: / })).toBeNull();
  });

  it("test19_a_folder_that_cannot_be_read_shows_a_card_and_a_way_to_try_again", async () => {
    resetPlusFixture({ down: true });
    localStorage.setItem("toolport-plus-context-folder", FIXTURE_FOLDER);
    render(<ContextScreen />);
    expect(await screen.findByText("This folder could not be read")).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent("toolportctl not found");
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });
});
