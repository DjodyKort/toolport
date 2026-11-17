import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { LibraryWorldOptions } from "../fixtures/libraryWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
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

const COVERS = ["library.remote", "library.sources", "library.taps"] as const;

function setup(library: LibraryWorldOptions = {}) {
  resetPlusFixture({ library });
  return render(<SourcesTab />);
}

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, Sources tab", () => {
  it("test00_every_covered_parity_action_is_built_and_names_this_file", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        test: "src/plus/library/SourcesTab.test.tsx",
      });
    }
  });

  it("test01_the_remote_card_shows_the_real_git_facts_and_why_pull_is_off", async () => {
    setup({ scenario: "plan" });
    const card = await screen.findByRole("region", { name: "Library remote" });
    await within(card).findByText(/Through git and gh/);
    expect(card).toHaveTextContent(
      "Through git and gh. Toolport never reads or stores them.",
    );
    expect(card).toHaveTextContent("0 ahead, 0 behind");
    expect(card).toHaveTextContent("2 uncommitted changes");
    expect(within(card).getByRole("button", { name: /Pull/ })).toBeDisabled();
    expect(card).toHaveTextContent("Pull is off: nothing to pull.");
  });

  it("test02_a_dirty_library_behind_its_remote_refuses_pull_and_says_to_commit_or_use_push", async () => {
    setup({ scenario: "behind", dirty: true });
    const card = await screen.findByRole("region", { name: "Library remote" });
    expect(await within(card).findByText("Behind and edited")).toBeVisible();
    expect(within(card).getByRole("button", { name: /Pull 3 commits/ })).toBeDisabled();
    expect(card).toHaveTextContent("commit or stash the 2 changes first, or use Push");
  });

  it("test03_pull_previews_the_fast_forward_and_runs_library_pull", async () => {
    setup({ scenario: "behind" });
    const card = await screen.findByRole("region", { name: "Library remote" });
    await userEvent.click(
      await within(card).findByRole("button", { name: /Pull 3 commits/ }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/Fast-forward 3 commit/)).toBeVisible();
    expect(ranExactly("library", "pull", "--dry-run")).toBe(true);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Pull and review plan" }),
    );
    await waitFor(() => expect(ranExactly("library", "pull")).toBe(true));
  });

  it("test04_push_shows_the_commits_and_the_scans_before_it_runs", async () => {
    setup({ scenario: "plan" });
    await userEvent.click(await screen.findByRole("button", { name: /Push…/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/commit 2 changed files/)).toBeVisible();
    expect(within(dialog).getByLabelText("Checks")).toHaveTextContent(
      "Secret scan: no findings",
    );
    expect(ranExactly("library", "push", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Push" }));
    await waitFor(() => expect(ranExactly("library", "push")).toBe(true));
  });

  it("test05_a_push_refused_by_the_secret_scan_names_rule_file_and_line_never_the_value", async () => {
    setup({ scenario: "plan", secret: true });
    await userEvent.click(await screen.findByRole("button", { name: /Push…/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("github-token")).toBeVisible();
    expect(dialog).toHaveTextContent("skills/deploy-notes/scripts/publish.sh:14");
    expect(dialog).toHaveTextContent("the value is never shown");
    expect(within(dialog).getByRole("button", { name: "Push" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Check again" })).toBeVisible();
  });

  it("test06_taps_show_their_health_update_with_a_retry_and_add_and_remove_after_a_preview", async () => {
    setup();
    const taps = await screen.findByRole("region", { name: "Taps" });
    expect(await within(taps).findByText("Update failed")).toBeVisible();
    expect(taps).toHaveTextContent("git asked for a password");
    await userEvent.click(within(taps).getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "update", "community-tools")).toBe(true),
    );
    expect(await within(taps).findAllByText(/^Updated /)).toHaveLength(1);

    await userEvent.click(within(taps).getByRole("button", { name: "Add tap…" }));
    await userEvent.type(screen.getByLabelText("Repository"), "acme-co/more-skills");
    await userEvent.click(screen.getByRole("button", { name: "Preview" }));
    await userEvent.click(await screen.findByRole("button", { name: "Add tap" }));
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "add", "acme-co/more-skills")).toBe(true),
    );
    expect(await within(taps).findByText("more-skills")).toBeVisible();

    await userEvent.click(
      within(taps).getByRole("button", { name: "More for more-skills" }),
    );
    await userEvent.click(await screen.findByRole("menuitem", { name: /Remove tap/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/cannot be undone/)).toBeVisible();
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove tap" }));
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "remove", "more-skills")).toBe(true),
    );
  });

  it("test07_find_and_install_audits_first_and_needs_a_tick_for_a_finding", async () => {
    setup();
    await userEvent.type(
      await screen.findByLabelText("Search skills in taps"),
      "deploy{Enter}",
    );
    const results = await screen.findByRole("list", { name: "Results" });
    expect(within(results).getByText("quick-deploy")).toBeVisible();
    await userEvent.click(within(results).getByRole("button", { name: "Install…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Tells the model not to mention a step/),
    ).toBeVisible();
    expect(
      ranExactly("skills", "install", "@community/toolbox/quick-deploy", "--dry-run"),
    ).toBe(true);
    const add = within(dialog).getByRole("button", { name: "Add to library" });
    expect(add).toBeDisabled();
    await userEvent.click(within(dialog).getByRole("checkbox"));
    expect(add).toBeEnabled();
    await userEvent.click(add);
    await waitFor(() =>
      expect(ranExactly("skills", "install", "@community/toolbox/quick-deploy")).toBe(
        true,
      ),
    );
  });

  it("test08_a_skill_already_in_the_library_is_marked_installed_and_a_clean_audit_needs_no_tick", async () => {
    setup();
    await userEvent.type(
      await screen.findByLabelText("Search skills in taps"),
      "a{Enter}",
    );
    const results = await screen.findByRole("list", { name: "Results" });
    await waitFor(() => expect(within(results).getByText("Installed")).toBeVisible());
    const row = within(results).getByText("dep-check").closest("li") as HTMLElement;
    await userEvent.click(within(row).getByRole("button", { name: "Install…" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/no risky instructions/)).toBeVisible();
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
  });

  it("test09_sources_are_one_filtered_list_and_only_your_library_is_writable", async () => {
    setup();
    const sources = await screen.findByRole("region", { name: "Sources" });
    await within(sources).findByText("skills-library");
    expect(within(sources).getByText("yours")).toBeVisible();
    expect(within(sources).getAllByText("read-only")).toHaveLength(9);
    expect(within(sources).getByText("duplicate clone")).toBeVisible();
    await userEvent.selectOptions(within(sources).getByLabelText("Source kind"), "tap");
    expect(within(sources).queryByText("skills-library")).toBeNull();
    expect(within(sources).getByText("acme-skills")).toBeVisible();
    expect(sources).toHaveTextContent("Only your own library is ever written");
    expect(ranExactly("sources", "ls")).toBe(true);
  });

  it("test10_a_folder_to_scan_is_previewed_added_and_removed", async () => {
    setup();
    await userEvent.click(
      await screen.findByRole("button", { name: /Add folder to scan/ }),
    );
    await userEvent.type(screen.getByLabelText("Folder"), "/Users/example/work/more");
    await userEvent.click(screen.getByRole("button", { name: "Preview" }));
    await userEvent.click(await screen.findByRole("button", { name: "Add folder" }));
    await waitFor(() =>
      expect(ranExactly("sources", "root", "add", "/Users/example/work/more")).toBe(true),
    );
    const list = await screen.findByRole("list", {
      name: "Folders you added to the scan",
    });
    await userEvent.click(
      within(list).getByRole("button", {
        name: "Stop scanning /Users/example/work/more",
      }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Stop scanning" }));
    await waitFor(() =>
      expect(ranExactly("sources", "root", "rm", "/Users/example/work/more")).toBe(true),
    );
    expect(ranExactly("sources", "root", "ls")).toBe(true);
  });
});
