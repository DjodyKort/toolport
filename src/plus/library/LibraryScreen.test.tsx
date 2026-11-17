import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import type { LibraryWorldOptions } from "../fixtures/libraryWorld";
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
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const COVERS = [
  "library.sync",
  "library.undo",
  "library.verify",
  "library.auto-sync",
  "library.checks",
  "library.items",
  "library.new",
  "library.bundle",
  "library.uninstall",
] as const;

function setup(library: LibraryWorldOptions = {}) {
  resetPlusFixture({ library });
  return render(<LibraryScreen />);
}

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

async function planReady() {
  await screen.findByText("8 changes can sync; 3 skills are held back");
}

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, Sync tab", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/library/LibraryScreen.test.tsx",
      });
    }
    expect(guiParity.routes.library).toMatchObject({
      status: "built",
      page: "view:library",
    });
  });

  it("test01_the_plan_checks_and_library_are_on_the_page_with_one_primary_button", async () => {
    setup();
    await planReady();
    expect(screen.getByRole("button", { name: "Apply 8 changes" })).toBeVisible();
    const plan = screen.getByRole("region", { name: "Sync plan" });
    expect(within(plan).getByText("build-triage")).toBeVisible();
    expect(within(plan).getByText("code-style")).toBeVisible();
    expect(within(plan).getByText(/task-runner · Stop/)).toBeVisible();
    const held = within(plan).getByRole("list", { name: "Held back" });
    expect(within(held).getAllByRole("listitem")).toHaveLength(3);
    expect(
      within(plan).getByText(/14 files written, 4 removed, 1 hook entry added/),
    ).toBeVisible();
    expect(screen.getByRole("tab", { name: /Lint/ })).toHaveTextContent("3 errors");
    expect(screen.getByRole("region", { name: "Library items" })).toHaveTextContent("34");
    expect(screen.getAllByRole("button", { name: /^Apply / })).toHaveLength(1);
    expect(ran("skills", "sync", "--dry-run")).toBe(true);
  });

  it("test02_apply_snapshots_writes_and_shows_the_verification_line_with_undo", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("button", { name: "Apply 8 changes" }));
    expect(
      await screen.findByText(
        "Synced 8 changes. Checked after writing: 183 of 183 library files match",
      ),
    ).toBeVisible();
    expect(ranExactly("skills", "sync", "--apply-held")).toBe(true);
    expect(screen.getByRole("button", { name: "Undo" })).toBeVisible();
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Sync plan" })).toHaveTextContent(
        "Nothing to write",
      ),
    );
  });

  it("test03_undo_previews_the_snapshot_restores_it_and_pauses_auto_sync", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("button", { name: "Apply 8 changes" }));
    await userEvent.click(await screen.findByRole("button", { name: "Undo" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/files removed/)).toBeVisible();
    expect(within(dialog).getByText(/Auto-sync is paused afterwards/)).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo sync" }));
    await waitFor(() => expect(ranExactly("skills", "undo")).toBe(true));
    expect(
      await screen.findByText(/Undone\. Claude Code is back to before/),
    ).toBeVisible();
    expect(screen.getByText(/\(after Undo\)/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(ranExactly("skills", "auto", "resume")).toBe(true));
  });

  it("test04_a_drift_after_writing_is_named_and_can_be_retried", async () => {
    setup({ drift: 2 });
    await planReady();
    await userEvent.click(screen.getByRole("button", { name: "Apply 8 changes" }));
    expect(
      await screen.findByText(/2 files did not match their library copy/),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry those 2" })).toBeVisible();
  });

  it("test05_auto_sync_shows_its_state_pauses_and_reports_what_it_would_hold", async () => {
    setup({ scenario: "held", auto: "on" });
    expect(
      await screen.findByText("The next auto-sync would stop before writing"),
    ).toBeVisible();
    expect(screen.getByText(/It would remove 4 files and add 1 hook/)).toBeVisible();
    expect(ran("skills", "sync", "--dry-run", "--unattended")).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: /Pause/ }));
    await waitFor(() => expect(ranExactly("skills", "auto", "pause")).toBe(true));
    await waitFor(() => expect(screen.getByText("Auto-sync paused")).toBeVisible());
    expect(screen.queryByText("The next auto-sync would stop before writing")).toBeNull();
    expect(screen.getByText(/The schedule stays on and skips its runs/)).toBeVisible();
  });

  it("test05a_auto_sync_that_is_off_says_so_and_turns_on_with_an_undo", async () => {
    setup();
    await planReady();
    const row = screen.getByTestId("auto-row");
    expect(within(row).getByText("Auto-sync off")).toBeVisible();
    expect(within(row).queryByRole("button", { name: /Pause/ })).toBeNull();
    expect(screen.queryByText("The next auto-sync would stop before writing")).toBeNull();
    expect(ran("skills", "sync", "--dry-run", "--unattended")).toBe(false);
    await userEvent.click(
      within(row).getByRole("button", { name: /^Turn on \(daily \d\d:\d\d\)$/ }),
    );
    await waitFor(() => expect(ranExactly("skills", "auto", "on")).toBe(true));
    await waitFor(() => expect(within(row).getByText(/^Auto-sync daily/)).toBeVisible());
    expect(within(row).getByText("no run yet")).toBeVisible();
    const [message, options] = vi.mocked(toast.message).mock.calls[0];
    expect(message).toBe("Auto-sync is on");
    (options as unknown as { action: { onClick: () => void } }).action.onClick();
    await waitFor(() => expect(ranExactly("skills", "auto", "off")).toBe(true));
    await waitFor(() => expect(within(row).getByText("Auto-sync off")).toBeVisible());
  });

  it("test05b_auto_sync_that_is_on_shows_when_it_last_ran_and_turns_off", async () => {
    setup({ auto: "on" });
    await planReady();
    const row = screen.getByTestId("auto-row");
    expect(within(row).getByText(/^Auto-sync daily \d\d:\d\d$/)).toBeVisible();
    expect(within(row).getByText("last run 3 h ago")).toBeVisible();
    expect(within(row).queryByText(/held/)).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Turn off" }));
    await waitFor(() => expect(ranExactly("skills", "auto", "off")).toBe(true));
    await waitFor(() => expect(within(row).getByText("Auto-sync off")).toBeVisible());
  });

  it("test05c_auto_sync_paused_after_an_undo_says_since_when_and_resumes", async () => {
    setup({ auto: "paused" });
    const row = await screen.findByTestId("auto-row");
    expect(within(row).getByText("Auto-sync paused")).toBeVisible();
    expect(within(row).getByText(/\(after Undo\)/)).toBeVisible();
    await userEvent.click(within(row).getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(ranExactly("skills", "auto", "resume")).toBe(true));
    await waitFor(() => expect(within(row).getByText(/^Auto-sync daily/)).toBeVisible());
  });

  it("test05d_what_the_last_auto_sync_held_is_counted_and_marked_in_the_plan", async () => {
    const reviewed = vi.fn();
    Element.prototype.scrollIntoView = reviewed;
    setup({ scenario: "held", auto: "held" });
    const row = await screen.findByTestId("auto-row");
    expect(
      await within(row).findByText("Last auto-sync held 5 changes — review in the plan"),
    ).toBeVisible();
    expect(screen.queryByText("The next auto-sync would stop before writing")).toBeNull();
    const plan = screen.getByRole("region", { name: "Sync plan" });
    expect(within(plan).getAllByText(/Held by the last auto-sync/)).toHaveLength(2);
    await userEvent.click(within(row).getByRole("button", { name: "Review plan" }));
    expect(reviewed).toHaveBeenCalled();
  });

  it("test05e_a_failed_auto_sync_says_what_went_wrong", async () => {
    setup({ auto: "failed" });
    const row = await screen.findByTestId("auto-row");
    expect(
      await within(row).findByText(/Last auto-sync failed: no skills repository found/),
    ).toBeVisible();
  });

  it("test06_the_states_the_library_can_be_in_each_have_their_own_card", async () => {
    const { unmount } = setup({ scenario: "empty" });
    expect(await screen.findByText("No skills library yet")).toBeVisible();
    expect(screen.getByRole("button", { name: "Start a local library" })).toBeVisible();
    unmount();
    const blocked = setup({ scenario: "blocked" });
    expect(await screen.findByText("Sync cannot run")).toBeVisible();
    expect(screen.getByText(/chmod -R u\+w/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Copy fix" })).toBeVisible();
    blocked.unmount();
    setup({ scenario: "busy" });
    expect(await screen.findByText("Another sync is running")).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  });

  it("test07_a_library_behind_its_remote_pulls_and_reviews_in_one_step", async () => {
    setup({ scenario: "behind" });
    await screen.findByText("Your library is 3 commits behind its remote");
    await userEvent.click(screen.getByRole("button", { name: /Pull and review plan/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/Fast-forward 3 commit/)).toBeVisible();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Pull and review plan" }),
    );
    await waitFor(() => expect(ranExactly("library", "pull")).toBe(true));
    expect(
      await screen.findByText("3 changes are waiting for Claude Code"),
    ).toBeVisible();
  });

  it("test08_a_plan_row_opens_to_every_file_with_a_status", async () => {
    setup();
    await planReady();
    const plan = screen.getByRole("region", { name: "Sync plan" });
    await userEvent.click(within(plan).getByRole("button", { name: /build-triage/ }));
    const files = await screen.findByRole("list", { name: "Files of build-triage" });
    expect(within(files).getByText("templates/report/.gitignore")).toBeVisible();
    expect(within(files).getAllByText("New")).toHaveLength(7);
    expect(ran("skills", "files", "build-triage")).toBe(true);
  });

  it("test09_checks_group_lint_by_what_claude_does_and_make_a_hook_script_executable", async () => {
    setup({ chmod: true });
    await screen.findByText(/skills are held back/);
    const hides = screen.getByLabelText("Claude Code hides these");
    expect(within(hides).getAllByRole("listitem")).toHaveLength(4);
    expect(
      screen.getByLabelText("Claude Code reads these, with a catch"),
    ).toHaveTextContent("build-triage");
    expect(screen.getByRole("img", { name: /1,602 characters/ })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Make executable" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/0644 → 0755/)).toBeVisible();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Make executable" }),
    );
    await waitFor(() =>
      expect(ranExactly("skills", "fix", "chmod", "task-runner")).toBe(true),
    );
  });

  it("test10_the_audit_and_collision_counters_open_their_own_lists", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("tab", { name: /Audit/ }));
    expect(
      await screen.findByText(/no risky instructions in 34 library items/),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("tab", { name: /Drift/ }));
    expect(screen.getAllByText(/was edited after the last sync/)).toHaveLength(2);
    expect(ran("skills", "audit")).toBe(true);
  });

  it("test11_a_skill_opens_as_a_page_with_every_file_its_links_and_the_never_copied_list", async () => {
    setup();
    await planReady();
    await userEvent.type(screen.getByLabelText("Search library"), "spec-writer");
    const list = screen.getByRole("region", { name: "Library items" });
    await userEvent.click(within(list).getByRole("button", { name: /spec-writer/ }));
    expect(await screen.findByRole("heading", { name: "spec-writer" })).toBeVisible();
    const files = screen.getByRole("region", { name: "Files" });
    expect(within(files).getByText("references/rubric.md")).toBeVisible();
    expect(within(files).getAllByText("Not copied")).toHaveLength(2);
    expect(within(files).getByLabelText("Never copied")).toHaveTextContent(".DS_Store");
    const links = screen.getByRole("region", { name: "Links" });
    expect(links).toHaveTextContent("Outside this skill folder");
    expect(links).toHaveTextContent("The file does not exist in your library");
    await userEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(await screen.findByRole("tab", { name: "Sync" })).toBeVisible();
  });

  it("test12_claude_on_off_is_read_from_the_skills_scan_and_the_list_filters", async () => {
    setup({ scenario: "done" });
    await screen.findByText("Claude Code has everything in your library");
    const list = screen.getByRole("region", { name: "Library items" });
    await waitFor(() =>
      expect(within(list).getAllByText("Claude: on").length).toBeGreaterThan(3),
    );
    expect(ran("skills", "ls", "--source", "library")).toBe(true);
    await userEvent.type(within(list).getByLabelText("Search library"), "screenshot");
    expect(within(list).getByText("Claude: off")).toBeVisible();
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
  });

  it("test13_check_now_verifies_every_library_file_against_its_installed_copy", async () => {
    setup({ scenario: "done" });
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(
      await screen.findByText(/Checked now: 183 of 183 library files match/),
    ).toBeVisible();
    expect(ran("skills", "verify")).toBe(true);
  });

  it("test14_turning_agents_md_on_previews_the_plan_with_that_client_first", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("switch", { name: "Sync to AGENTS.md" }));
    await waitFor(() =>
      expect(
        ranExactly(
          "skills",
          "sync",
          "--dry-run",
          "--client",
          "claude-code",
          "--client",
          "agents-md",
        ),
      ).toBe(true),
    );
    expect(screen.queryByRole("switch", { name: "Sync to Cursor" })).toBeNull();
  });

  it("test15_first_sync_after_the_update_needs_a_tick_for_files_that_already_differ", async () => {
    setup({ scenario: "catchup" });
    await screen.findByText(/none are in Claude Code yet/);
    const tick = screen.getByRole("checkbox", {
      name: /that already exist with other content/,
    });
    expect(screen.getByRole("button", { name: "Apply 6 changes" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Apply 6 changes" }));
    await waitFor(() => expect(ranExactly("skills", "sync")).toBe(true));
    expect(tick).not.toBeNull();
  });

  it("test16_new_skill_and_the_bundle_menu_preview_before_they_write", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("button", { name: "New skill…" }));
    await userEvent.type(screen.getByLabelText("Name"), "fresh-skill");
    await userEvent.click(screen.getByRole("button", { name: "Preview" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/SKILL\.md/)).toBeVisible();
    expect(ranExactly("skills", "add", "fresh-skill", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(ranExactly("skills", "add", "fresh-skill")).toBe(true));

    await userEvent.click(await screen.findByRole("button", { name: "More" }));
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Export bundle…" }),
    );
    const bundle = await screen.findByRole("dialog");
    expect(await within(bundle).findByText(/skills-library-bundle\.zip/)).toBeVisible();
    await userEvent.click(within(bundle).getByRole("button", { name: "Export bundle" }));
    await waitFor(() => expect(ranExactly("skills", "bundle")).toBe(true));
  });

  it("test17_import_bundle_asks_for_the_zip_then_previews_it", async () => {
    setup();
    await planReady();
    await userEvent.click(screen.getByRole("button", { name: "More" }));
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "Import bundle…" }),
    );
    await userEvent.type(screen.getByLabelText("Path of the zip"), "/tmp/a.zip");
    await userEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(await screen.findByText(/Adds 1 item: imported-skill/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Import bundle" }));
    await waitFor(() =>
      expect(ranExactly("skills", "unbundle", "/tmp/a.zip")).toBe(true),
    );
  });

  it("test18_delete_from_library_names_the_files_and_runs_uninstall_after_the_preview", async () => {
    setup();
    await planReady();
    await userEvent.type(screen.getByLabelText("Search library"), "doc-outline");
    const list = screen.getByRole("region", { name: "Library items" });
    await userEvent.click(within(list).getByRole("button", { name: /doc-outline/ }));
    await userEvent.click(await screen.findByRole("button", { name: "More" }));
    await userEvent.click(
      await screen.findByRole("menuitem", { name: /Delete from library/ }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/cannot be recovered from git/)).toBeVisible();
    expect(ranExactly("skills", "uninstall", "doc-outline", "--dry-run")).toBe(true);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Delete from library" }),
    );
    await waitFor(() =>
      expect(ranExactly("skills", "uninstall", "doc-outline")).toBe(true),
    );
    expect(await screen.findByRole("tab", { name: "Sync" })).toBeVisible();
  });

  it("test19_the_plugins_tab_says_it_is_planned_and_the_tabs_move_with_the_arrow_keys", async () => {
    setup();
    await planReady();
    const sync = screen.getByRole("tab", { name: "Sync" });
    sync.focus();
    await userEvent.keyboard("{ArrowRight}{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Plugins" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByText("Planned")).toBeVisible();
    expect(screen.getByText(/arrive in a later slice/)).toBeVisible();
  });

  it("test20_nothing_in_the_screen_offers_the_removed_targets", async () => {
    setup();
    await planReady();
    expect(screen.queryByText(/12 other clients/)).toBeNull();
    const targets = screen.getByRole("region", { name: "Targets" });
    expect(within(targets).getAllByRole("switch")).toHaveLength(2);
  });
});
