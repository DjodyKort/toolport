import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import type { CompressionWorldOptions } from "../fixtures/compressionWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import HooksCompression from "./HooksCompression";

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

const COVERS = [
  "compression.hooks",
  "compression.status",
  "compression.doctor",
  "compression.measure",
] as const;

function setup(compression: CompressionWorldOptions = {}) {
  resetPlusFixture({ compression });
  return render(<HooksCompression />);
}

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

const undoOf = (call = -1) => {
  const calls = vi.mocked(toast.success).mock.calls;
  const options = calls.at(call)?.[1] as { action?: { onClick: () => void } } | undefined;
  return options?.action;
};

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Hooks & compression tab", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/compression/HooksCompression.test.tsx",
      });
    }
    expect(guiParity.routes.compression).toMatchObject({ status: "built" });
  });

  it("test01_a_first_run_offers_to_adopt_the_two_hand_wired_hooks", async () => {
    setup();
    expect(
      await screen.findByText("Toolport found 2 compression hooks it did not write"),
    ).toBeVisible();
    const found = screen.getByRole("region", { name: "Found in settings.json" });
    expect(within(found).getAllByRole("button", { name: /^Adopt / })).toHaveLength(2);
    expect(
      within(screen.getByRole("region", { name: "Providers" })).getAllByRole("button", {
        name: /Show why parsec is locked|Why headroom is off/,
      }),
    ).toHaveLength(2);
    expect(screen.getByText("Inventory not available in this build")).toBeVisible();
  });

  it("test02_preview_adoption_shows_the_lines_that_change_and_writes_nothing", async () => {
    setup();
    await userEvent.click(
      await screen.findByRole("button", { name: "Preview adoption…" }),
    );
    const preview = await screen.findByTestId("adopt-preview");
    expect(preview).toHaveTextContent("rtk in ~/.claude/settings.json: wrapped");
    expect(preview).toHaveTextContent("− rtk hook claude");
    expect(preview).toHaveTextContent(
      "+ toolportctl compression hook rtk -- /bin/sh -c 'rtk hook claude'",
    );
    expect(ranArgv()).toContainEqual(["compression", "hooks", "apply", "--dry-run"]);
    expect(ranArgv()).not.toContainEqual(["compression", "hooks", "apply"]);
    expect(
      screen.getByText("Toolport found 2 compression hooks it did not write"),
    ).toBeVisible();
  });

  it("test03_adopt_is_one_click_with_undo_and_no_dialog", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Adopt" }));
    expect(
      await screen.findByText(
        "Compression is on: 2 hooks written by Toolport, all healthy",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(ranArgv()).toContainEqual(["compression", "hooks", "apply"]);
    expect(toast.success).toHaveBeenCalledWith(
      "Adopted rtk and read-once",
      expect.objectContaining({ duration: 10_000 }),
    );
    expect(undoOf()?.onClick).toBeDefined();

    undoOf()!.onClick();
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "undo",
        "--provider",
        "read-once",
      ]),
    );
    expect(ranArgv()).toContainEqual([
      "compression",
      "hooks",
      "undo",
      "--provider",
      "rtk",
    ]);
    expect(
      await screen.findByText("Toolport found 2 compression hooks it did not write"),
    ).toBeVisible();
  });

  it("test04_a_hook_just_adopted_has_no_counted_calls_yet", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Adopt" }));
    expect(await screen.findByTestId("hook-detail-rtk")).toHaveTextContent(
      "No calls counted yet. The hook counts from the next Claude Code session.",
    );
  });

  it("test05_adopting_one_hook_leaves_the_other_found_and_undo_restores_it", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Adopt rtk" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "apply",
        "--provider",
        "rtk",
      ]),
    );
    const written = await screen.findByRole("region", { name: "Written by Toolport" });
    expect(within(written).getByText("rtk")).toBeVisible();
    expect(
      within(screen.getByRole("region", { name: "Found in settings.json" })).getByText(
        "read-once",
      ),
    ).toBeVisible();
    undoOf()!.onClick();
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "undo",
        "--provider",
        "rtk",
      ]),
    );
  });

  it("test06_hooks_that_are_on_show_their_counted_calls_and_latency", async () => {
    setup({ hooks: "on" });
    expect(
      await screen.findByText(
        "Compression is on: 2 hooks written by Toolport, all healthy",
      ),
    ).toBeVisible();
    expect(screen.getByTestId("hook-detail-rtk")).toHaveTextContent(
      "Answered 1,842 of 2,100 calls since 2026-11-05 · median 38 ms · p95 140 ms · 3 failed (counted at the hook)",
    );
    expect(screen.getByTestId("hook-detail-read-once")).toHaveTextContent(
      "Answered 312 of 340 calls",
    );
  });

  it("test07_pause_and_resume_are_one_click_with_undo_and_a_pause_never_resumes_silently", async () => {
    setup({ hooks: "on" });
    await userEvent.click(
      await screen.findByRole("button", { name: "Pause compression" }),
    );
    expect(await screen.findByText(/Compression is paused since/)).toBeVisible();
    expect(screen.getByText(/Nothing resumes by itself/)).toBeVisible();
    expect(ranArgv()).toContainEqual(["compression", "hooks", "pause"]);
    undoOf()!.onClick();
    await waitFor(() => expect(ran("hooks", "resume")).toBe(true));
    expect(
      await screen.findByText(
        "Compression is on: 2 hooks written by Toolport, all healthy",
      ),
    ).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Pause compression" }));
    await userEvent.click(await screen.findByRole("button", { name: "Resume now" }));
    await waitFor(() =>
      expect(ranArgv().filter((a) => a.includes("resume"))).toHaveLength(2),
    );
  });

  it("test08_a_missing_hook_asks_to_be_re_applied_and_says_nothing_errors", async () => {
    setup({ hooks: "missing" });
    expect(await screen.findByText("A compression hook needs you: rtk")).toBeVisible();
    expect(screen.getByText("missing from settings.json")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Re-apply rtk" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "apply",
        "--provider",
        "rtk",
      ]),
    );
    expect(
      await screen.findByText(
        "Compression is on: 2 hooks written by Toolport, all healthy",
      ),
    ).toBeVisible();
    expect(undoOf()).toBeUndefined();
  });

  it("test09_an_edited_hook_can_be_restored_to_what_toolport_wrote", async () => {
    setup({ hooks: "edited" });
    expect(await screen.findByText("edited by hand")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Restore ours for rtk" }));
    expect(
      await screen.findByText(
        "Compression is on: 2 hooks written by Toolport, all healthy",
      ),
    ).toBeVisible();
  });

  it("test10_no_hook_at_all_says_toolport_does_not_install_binaries", async () => {
    setup({ hooks: "none" });
    expect(await screen.findByText("No compression hooks found")).toBeVisible();
    expect(screen.getByText(/does not install third-party binaries/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Adopt" })).toBeNull();
  });

  it("test11_verify_all_reads_the_entries_and_runs_the_policy_checks", async () => {
    setup({ hooks: "on" });
    await userEvent.click(await screen.findByRole("button", { name: "Verify all" }));
    const result = await screen.findByTestId("verification");
    expect(result).toHaveTextContent(/Checked at \d{2}:\d{2}/);
    expect(result).toHaveTextContent("rtk: the entry reads as Toolport wrote it");
    expect(result).toHaveTextContent("engine: compression disabled (provider none)");
    expect(ranArgv()).toContainEqual(["compression", "doctor"]);
  });

  it("test12_undo_adoption_on_a_card_restores_the_original_and_offers_to_adopt_again", async () => {
    setup({ hooks: "on" });
    await userEvent.click(
      await screen.findByRole("button", { name: "Undo adoption of rtk" }),
    );
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "undo",
        "--provider",
        "rtk",
      ]),
    );
    expect(await screen.findByRole("button", { name: "Adopt rtk" })).toBeVisible();
    undoOf()!.onClick();
    await waitFor(() =>
      expect(ranArgv()).toContainEqual([
        "compression",
        "hooks",
        "apply",
        "--provider",
        "rtk",
      ]),
    );
  });

  it("test13_parsec_is_locked_behind_a_consent_flag_and_the_proxy_says_why_it_is_off", async () => {
    setup({ hooks: "on" });
    await userEvent.click(
      await screen.findByRole("button", { name: "Show why parsec is locked" }),
    );
    expect(
      screen.getByText(
        "toolportctl compression enable --provider parsec --accept-egress parsec",
      ),
    ).toBeVisible();
    expect(screen.getByText(/Sends your code off this machine\./)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Why headroom is off" }));
    expect(screen.getByText(/can break the prompt-cache prefix/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Show engine controls" }));
    const controls = await screen.findByTestId("engine-controls");
    expect(controls).toHaveTextContent("toolportctl compression proxy restart");
    expect(controls).toHaveTextContent("0.29.0, not installed");
  });

  it("test14_a_failed_read_says_so_and_offers_to_try_again", async () => {
    resetPlusFixture({ down: true });
    render(<HooksCompression />);
    expect(await screen.findByText("The hook state could not be read")).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });

  it("test15_not_now_hides_the_adopt_card_and_keeps_the_per_hook_buttons", async () => {
    setup();
    await userEvent.click(await screen.findByRole("button", { name: "Not now" }));
    expect(
      screen.queryByText("Toolport found 2 compression hooks it did not write"),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Adopt rtk" })).toBeVisible();
  });
});

describe("Compression measurements", () => {
  async function open(options: CompressionWorldOptions = { hooks: "on" }) {
    setup(options);
    await userEvent.click(
      await screen.findByRole("button", { name: "See measurements" }),
    );
    await screen.findByRole("heading", { name: "Compression measurements" });
  }

  it("test16_the_page_has_four_cards_each_with_its_population_and_what_it_cannot_tell", async () => {
    await open();
    expect(ranArgv()).toContainEqual(["compression", "measure"]);
    for (const title of [
      "1. Where context tokens come from",
      "2. What the hooks did",
      "3. Billed outcome, by day",
      "4. Rework check",
    ]) {
      const card = await screen.findByRole("region", { name: title });
      expect(within(card).getByText("Population:")).toBeVisible();
    }
    expect(screen.getAllByText("Cannot tell you:")).toHaveLength(4);
    expect(screen.queryByText(/saved \d/)).toBeNull();
  });

  it("test17_the_sources_show_shares_and_name_bash_as_a_ceiling_not_a_saving", async () => {
    await open();
    const card = await screen.findByRole("region", {
      name: "1. Where context tokens come from",
    });
    expect(within(card).getByText("38%")).toBeVisible();
    expect(within(card).getByTestId("bash-ceiling")).toHaveTextContent(
      "At most 27% of context-turns came from Bash output.",
    );
    expect(within(card).getByTestId("bash-ceiling")).toHaveTextContent(
      "an opportunity bound, not a saving",
    );
    expect(card).toHaveTextContent(
      "461 sessions on this machine, 2026-10-16 to 2026-11-16",
    );
  });

  it("test18_the_hook_card_counts_calls_and_latency_and_says_bytes_are_not_counted", async () => {
    await open();
    const card = await screen.findByRole("region", { name: "2. What the hooks did" });
    expect(card).toHaveTextContent("1,842 answered of 2,100 calls");
    expect(card).toHaveTextContent("p50 38 ms · p95 140 ms");
    expect(within(card).getByTestId("output-bytes")).toHaveTextContent(
      "Bytes saved: not counted.",
    );
    expect(card).toHaveTextContent("2 hooks, 2026-11-05 to 2026-11-16, 2,440 calls");
  });

  it("test19_a_thin_paused_window_gets_a_refusal_instead_of_a_percentage", async () => {
    await open();
    expect(await screen.findByTestId("verdict")).toHaveTextContent(
      "Needs at least 8 sessions with 5 or more turns in each window; hooks on has 118, paused has 4.",
    );
    expect(screen.getByText("No verdict")).toBeVisible();
    expect(screen.getByTestId("window-on")).toHaveTextContent(
      "118 sessions · median cost 100 · middle half 71 to 160",
    );
    expect(screen.getByTestId("window-paused")).toHaveTextContent(
      "4 sessions · median cost 112",
    );
    expect(screen.getByTestId("timeline")).toHaveTextContent(
      "Hooks off before 2026-11-05 · Hooks on 2026-11-05 to 2026-11-12 · Paused 2026-11-12 to 2026-11-13 · Hooks on since 2026-11-13",
    );
  });

  it("test20_enough_paused_sessions_give_a_comparison_that_says_when_halves_overlap", async () => {
    await open({ hooks: "on", measure: "compared" });
    expect(await screen.findByTestId("verdict")).toHaveTextContent(
      "The middle halves of both windows overlap",
    );
    expect(screen.getByText("Compared")).toBeVisible();
  });

  it("test21_the_rework_card_shows_repeats_per_session_and_warns_about_few_sessions", async () => {
    await open();
    const card = await screen.findByRole("region", { name: "4. Rework check" });
    expect(card).toHaveTextContent("Hooks on");
    expect(within(card).getAllByText("3.4")).toHaveLength(1);
    expect(card).toHaveTextContent("treat both figures as hints");
  });

  it("test22_the_upstream_figure_stays_apart_from_every_card", async () => {
    await open();
    expect(await screen.findByTestId("upstream-note")).toHaveTextContent(
      "Upstream's number stays separate.",
    );
  });

  it("test23_without_transcripts_each_card_degrades_alone", async () => {
    await open({ hooks: "on", measure: "empty" });
    expect(await screen.findByText("No transcripts yet")).toBeVisible();
    expect(screen.getAllByTestId("card-empty")).toHaveLength(4);
    expect(
      screen.getByText(/Question 2 starts counting after the next hook call/),
    ).toBeVisible();
  });

  it("test24_back_returns_to_the_hooks_and_recompute_runs_the_measurement_again", async () => {
    await open();
    await userEvent.click(screen.getByRole("button", { name: "Recompute" }));
    await waitFor(() =>
      expect(ranArgv().filter((a) => a.includes("measure"))).toHaveLength(2),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Back to Hooks and compression" }),
    );
    expect(await screen.findByRole("region", { name: "Providers" })).toBeVisible();
  });
});
