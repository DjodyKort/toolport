import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import type { WiringWorldOptions } from "../fixtures/wiringWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import RulesLaunch from "./RulesLaunch";

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

const HOME = "/Users/example";

function setup(wiring: WiringWorldOptions = {}, extra: Record<string, unknown> = {}) {
  resetPlusFixture({ wiring, ...extra });
  return render(<RulesLaunch home={HOME} folder={null} />);
}

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

const exactly = (...argv: string[]) =>
  ranArgv().some((run) => run.join(" ") === argv.join(" "));

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Rules and launch", () => {
  it("test00_the_parity_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["context.rules-launch"]).toMatchObject({
      status: "built",
      test: "src/plus/context/RulesLaunch.test.tsx",
    });
  });

  it("test01_a_healthy_wiring_is_one_calm_card_with_the_last_run_and_the_checks", async () => {
    setup();
    expect(await screen.findByText("Launch wiring is healthy")).toBeVisible();
    expect(
      screen.getByText(/Last run .* · next after .* · 1\.8 s, no warnings/),
    ).toBeVisible();
    expect(screen.getByText("context-shims is sourced last")).toBeVisible();
    expect(
      screen.getByText(
        "The org tool is installed: no coexistence constraints are violated",
      ),
    ).toBeVisible();
    expect(ran("context", "wiring", "status")).toBe(true);
    expect(screen.queryByText(/not part of this build/)).toBeNull();
    expect(screen.queryByText("Not built yet")).toBeNull();
  });

  it("test02_a_machine_that_is_not_set_up_offers_a_plan_and_changes_nothing_before_the_confirmation", async () => {
    setup({ state: "not_set_up" });
    expect(await screen.findByText("Set up the launch wiring")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Preview the setup…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("Set up the launch wiring", { selector: "p" }),
    ).toBeVisible();
    expect(
      within(dialog).getByText(/Adds one line at the end of the file/),
    ).toBeVisible();
    expect(exactly("context", "wiring", "init", "--dry-run")).toBe(true);
    expect(exactly("context", "wiring", "init")).toBe(false);
    await userEvent.click(within(dialog).getByRole("button", { name: "Set up" }));
    await waitFor(() => expect(exactly("context", "wiring", "init")).toBe(true));
    expect(await screen.findByText("Launch wiring is healthy")).toBeVisible();
  });

  it("test03_undo_after_the_setup_runs_disable", async () => {
    setup({ state: "not_set_up" });
    await userEvent.click(
      await screen.findByRole("button", { name: "Preview the setup…" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Set up" }));
    await waitFor(() => expect(toast.success).toHaveBeenCalled());
    const [, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [
      string,
      { action: { label: string; onClick: () => void } },
    ];
    expect(options.action.label).toBe("Undo");
    options.action.onClick();
    await waitFor(() => expect(exactly("context", "wiring", "disable")).toBe(true));
    expect(await screen.findByText("Set up the launch wiring")).toBeVisible();
  });

  it("test04_not_now_hides_the_offer_and_nothing_is_written", async () => {
    setup({ state: "not_set_up" });
    await userEvent.click(await screen.findByRole("button", { name: "Not now" }));
    expect(screen.queryByText("Set up the launch wiring")).toBeNull();
    expect(ran("wiring", "init")).toBe(false);
  });

  it("test05_a_source_order_finding_names_the_cause_and_fixes_it_after_a_plan", async () => {
    setup({ state: "look" });
    expect(
      await screen.findAllByText(
        "context-shims sourced BEFORE shell-wrapper.sh in ~/.zshrc",
      ),
    ).not.toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Show lines" }));
    expect(screen.getByText(/shell-wrapper\.sh is sourced after it/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Fix source order…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Moves our line to the end of the file/),
    ).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Move the line" }));
    await waitFor(() => expect(exactly("context", "wiring", "init")).toBe(true));
    expect(await screen.findByText("Launch wiring is healthy")).toBeVisible();
  });

  it("test06_a_failed_presync_shows_the_last_good_run_the_output_and_a_way_to_try_again", async () => {
    setup({ state: "failed" });
    expect(await screen.findByText("The last presync failed")).toBeVisible();
    expect(screen.getByText(/Layers on disk are from the last good run/)).toBeVisible();
    expect(
      screen.getByText(/A failed presync only warns; Claude still starts/),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Show output" }));
    expect(screen.getByText(/fatal: unable to access/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(exactly("context", "presync", "--force")).toBe(true));
    expect(await screen.findByText("Launch wiring is healthy")).toBeVisible();
  });

  it("test07_turning_the_wiring_off_names_what_is_removed_and_asks_first", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Turn off…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Removes the line marked toolport:context-shims/),
    ).toBeVisible();
    expect(exactly("context", "wiring", "disable")).toBe(false);
    await userEvent.click(within(dialog).getByRole("button", { name: "Turn off" }));
    await waitFor(() => expect(exactly("context", "wiring", "disable")).toBe(true));
    expect(await screen.findByText("Set up the launch wiring")).toBeVisible();
  });

  it("test08_what_a_sync_would_write_lists_layers_with_the_hand_edited_copy_flagged", async () => {
    setup();
    const card = (await screen.findByText("What a sync would write now")).closest(
      "section",
    )!;
    expect(
      await within(card).findByText(
        /Layer client-alpha → ~\/work\/clients\/alpha\/app-5\/CLAUDE\.local\.md/,
      ),
    ).toBeVisible();
    expect(
      within(card).getByText(
        /Layer client-beta in ~\/work\/clients\/beta\/api\/CLAUDE\.local\.md/,
      ),
    ).toBeVisible();
    expect(
      within(card).getByText("A hand-edited copy is replaced; a copy of it is kept."),
    ).toBeVisible();
    expect(exactly("context", "sync", "--dry-run", "--rules")).toBe(true);
  });

  it("test09_the_rules_switch_leaves_the_plan_without_the_global_layers", async () => {
    setup();
    await screen.findByText("What a sync would write now");
    await userEvent.click(
      screen.getByRole("switch", { name: "Deploy rules to ~/.claude/rules" }),
    );
    await waitFor(() => expect(exactly("context", "sync", "--dry-run")).toBe(true));
  });

  it("test10_sync_now_shows_the_plan_again_and_writes_after_one_confirmation_with_an_undo_for_the_layers", async () => {
    setup();
    await screen.findByText("What a sync would write now");
    await userEvent.click(screen.getAllByRole("button", { name: "Sync now…" })[0]);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Sync now?")).toBeVisible();
    expect(exactly("context", "sync", "--rules")).toBe(false);
    await userEvent.click(within(dialog).getByRole("button", { name: "Sync" }));
    await waitFor(() => expect(exactly("context", "sync", "--rules")).toBe(true));
    await waitFor(() => expect(toast.success).toHaveBeenCalled());
    const [title, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [
      string,
      { action: { label: string; onClick: () => void } },
    ];
    expect(title).toBe("Synced 2 changes");
    expect(options.action.label).toBe("Undo layers");
    options.action.onClick();
    await waitFor(() => expect(exactly("context", "layers", "undo")).toBe(true));
  });

  it("test11_layers_show_scope_delivery_state_and_where_they_land", async () => {
    setup();
    const alpha = (await screen.findByText("client-alpha")).closest(
      "div.flex-col",
    ) as HTMLElement;
    expect(within(alpha).getByText("folder pattern **/clients/alpha/**")).toBeVisible();
    expect(within(alpha).getByText("deployed")).toBeVisible();
    expect(
      within(alpha).getByText(
        "text copied into the layer · lands in CLAUDE.local.md in 4 folders",
      ),
    ).toBeVisible();
    const personal = screen.getByText("personal").closest("div.flex-col") as HTMLElement;
    expect(within(personal).getByText("everywhere")).toBeVisible();
    expect(
      within(personal).getByText(
        "text copied into the layer · lands in ~/.claude/rules/personal.md",
      ),
    ).toBeVisible();
    const beta = screen.getByText("client-beta").closest("div.px-3") as HTMLElement;
    expect(within(beta).getByText("hand-edited copy")).toBeVisible();
    expect(
      within(beta).getByText(/import · lands in CLAUDE\.local\.md in 2 folders/),
    ).toBeVisible();
    await userEvent.click(await within(beta).findByRole("button", { name: "Show diff" }));
    expect(within(beta).getByText("Now in the file")).toBeVisible();
    expect(within(beta).getByText(/Use the sandbox, mine\./)).toBeVisible();
  });

  it("test12_a_hand_written_rule_file_is_replaced_only_after_a_confirmation_or_kept", async () => {
    const layers = [
      {
        name: "personal",
        path: `${HOME}/.config/mcpm/skills_repo/rules/personal/SKILL.md`,
        scope: "global" as const,
        delivery: "copy" as const,
        globs: [],
        folderPatterns: [],
        imports: [],
        tokens: { value: 1860, basis: "estimate" as const },
        problems: [],
        state: "shadowed" as const,
        detail:
          "A file at the destination was not written by Toolport. A sync would replace it and keep a backup.",
        target: { kind: "rules" as const, file: `${HOME}/.claude/rules/personal.md` },
      },
    ];
    setup({ layers });
    expect(await screen.findByText("hand-written file in the way")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    await waitFor(() =>
      expect(ran("skills", "keep", "personal", "personal.md")).toBe(true),
    );
    await userEvent.click(screen.getByRole("button", { name: "Back up and replace" }));
    const dialog = await screen.findByRole("dialog");
    expect(ran("skills", "sync", "--take")).toBe(false);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Back up and replace" }),
    );
    await waitFor(() =>
      expect(
        exactly("skills", "sync", "--only", "personal", "--take", "personal/personal.md"),
      ).toBe(true),
    );
  });

  it("test13_a_layer_that_matches_no_folder_says_so", async () => {
    const [, , alpha] = (await import("../fixtures/wiringWorld")).seedLayers();
    setup({
      layers: [
        {
          ...alpha,
          state: "nowhere",
          detail:
            "No folder matches this layer's folders, so it is deployed nowhere. Fix the pattern or delete the layer.",
          target: { kind: "folders", folders: [], deployed: 0, file: "CLAUDE.local.md" },
        },
      ],
    });
    expect(await screen.findByText("lands nowhere")).toBeVisible();
    expect(screen.getByText(/deployed nowhere/)).toBeVisible();
  });

  it("test14_the_org_file_is_read_only_and_a_missing_org_tool_is_a_fact_not_a_problem", async () => {
    setup();
    const org = (await screen.findByText("Org file")).closest("section")!;
    expect(within(org).getByText("read-only")).toBeVisible();
    expect(within(org).getByText("corp-tools")).toBeVisible();
    expect(within(org).getByText(/Toolport never edits this file/)).toBeVisible();
    expect(within(org).getByText(/≈ 10,070 tokens, estimate/)).toBeVisible();
  });

  it("test14b_the_org_card_shows_the_pull_command_as_text_and_says_where_to_change_it", async () => {
    setup();
    const org = (await screen.findByText("Org file")).closest("section")!;
    expect(
      within(org).getByText("git pull, then the org tool's own sync (built in)"),
    ).toBeVisible();
    expect(
      within(org).getByText(/Nothing is set, so the built-in pull runs/),
    ).toBeVisible();
    expect(within(org).getByText("orgPull.command")).toBeVisible();
    expect(within(org).getByText("~/.config/mcpm/context.json")).toBeVisible();
    expect(within(org).queryByRole("textbox")).toBeNull();
    expect(within(org).queryByRole("button")).toBeNull();
  });

  it("test14c_a_configured_command_is_shown_as_it_runs", async () => {
    setup({ orgPullCommand: ["org-pull", "--quiet", "two words"] });
    const org = (await screen.findByText("Org file")).closest("section")!;
    expect(within(org).getByText('org-pull --quiet "two words"')).toBeVisible();
    expect(within(org).queryByText(/Nothing is set/)).toBeNull();
    expect(within(org).getByText(/the pull command sync, about every 4 h/)).toBeVisible();
  });

  it("test14d_an_unusable_setting_shows_the_reason_on_the_card", async () => {
    setup({
      orgPullError: "orgPull in context.json: command is empty; give the program to run",
    });
    const org = (await screen.findByText("Org file")).closest("section")!;
    expect(
      within(org).getByText(/command is empty; give the program to run/),
    ).toBeVisible();
    expect(within(org).getByText("not run until context.json is fixed")).toBeVisible();
  });

  it("test15_without_the_org_tool_the_card_says_nothing_is_wrong", async () => {
    setup({ org: false });
    expect(
      (await screen.findAllByText("The org tool is not installed")).length,
    ).toBeGreaterThan(1);
    expect(
      screen.getAllByText(/No org file loads and no coexistence rules apply/)[0],
    ).toBeVisible();
  });

  it("test16_the_gauge_shows_the_session_the_window_and_the_checkpoint_from_the_transcripts", async () => {
    setup({ checkpointAt: 44_000 });
    const gauge = await screen.findByRole("meter", { name: "Context used" });
    expect(gauge).toHaveAttribute("aria-valuenow", "142000");
    expect(gauge).toHaveAttribute("aria-valuemax", "200000");
    expect(
      screen.getByText(
        /142,000 of 200,000 used in the newest session .* checkpoint at 156,000 · compaction at 200,000\. Window from the model name\./,
      ),
    ).toBeVisible();
    expect(ran("context", "checkpoint-status")).toBe(true);
  });

  it("test17_without_a_session_the_gauge_says_so_and_shows_no_number", async () => {
    setup({ noSession: true });
    expect(
      await screen.findByText(/No Claude session with usage was found yet/),
    ).toBeVisible();
    expect(screen.queryByRole("meter")).toBeNull();
  });

  it("test18_the_checkpoint_is_set_after_a_plan_and_undone_with_the_toast", async () => {
    setup();
    await screen.findByRole("meter", { name: "Context used" });
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    const input = await screen.findByLabelText("Tokens before compaction");
    await userEvent.type(input, "44000");
    await userEvent.click(screen.getByRole("button", { name: "Preview the plan" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("Checkpoint: 44000 tokens before compaction"),
    ).toBeVisible();
    expect(exactly("context", "checkpoint-config", "--at", "44000", "--dry-run")).toBe(
      true,
    );
    await userEvent.click(within(dialog).getByRole("button", { name: "Set checkpoint" }));
    await waitFor(() =>
      expect(exactly("context", "checkpoint-config", "--at", "44000")).toBe(true),
    );
    expect(await screen.findByText(/checkpoint at 156,000/)).toBeVisible();
    const [, options] = vi.mocked(toast.success).mock.calls[0] as unknown as [
      string,
      { action: { onClick: () => void } },
    ];
    options.action.onClick();
    await waitFor(() =>
      expect(exactly("context", "checkpoint-config", "--off")).toBe(true),
    );
  });

  it("test19_a_checkpoint_outside_the_range_cannot_be_previewed", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    await userEvent.type(
      await screen.findByLabelText("Tokens before compaction"),
      "2000000",
    );
    expect(screen.getByRole("alert")).toHaveTextContent("between 1 and 999,999");
    expect(screen.getByRole("button", { name: "Preview the plan" })).toBeDisabled();
  });

  it("test20_the_shell_functions_are_listed_read_only_once_the_wiring_is_set_up", async () => {
    setup();
    const group = (await screen.findByText("Shell functions")).closest("section")!;
    for (const name of ["context_presync", "claude", "claude-bare", "hrclaude"])
      expect(within(group).getByText(name)).toBeVisible();
    expect(within(group).getByText(/Generated file, read-only/)).toBeVisible();
  });

  it("test21_before_the_setup_the_functions_say_they_come_with_it", async () => {
    setup({ state: "not_set_up" });
    await screen.findByText("Set up the launch wiring");
    expect(screen.getByText(/Set the launch wiring up to get them/)).toBeVisible();
  });

  it("test22_a_bridge_that_is_down_shows_a_card_with_a_way_to_try_again", async () => {
    resetPlusFixture({ down: true });
    render(<RulesLaunch home={HOME} folder={null} />);
    expect(await screen.findByText("The launch wiring could not be read")).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });
});
