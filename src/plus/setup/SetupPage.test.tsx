import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import {
  exportArgv,
  NO_CHOICE,
  restoreArgv,
  sinceLine,
  type ExportChoice,
} from "./model";
import { SetupPage } from "./SetupPage";

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
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

/** The actions of src/plus/gui-parity/ this file proves. */
const COVERS = ["setup.export", "setup.restore", "setup.undo", "setup.check"] as const;
const PASS = "a long enough passphrase";
const FILE = "/tmp/setup.zip";

const choice = (over: Partial<ExportChoice>): ExportChoice => ({ ...NO_CHOICE, ...over });

function lastExportArgv(): string[] {
  return ranArgv()
    .filter((a) => a[1] === "export")
    .at(-1)!;
}

async function openRestore(user: ReturnType<typeof userEvent.setup>) {
  render(<SetupPage onBack={() => {}} />);
  await user.click(screen.getByRole("tab", { name: "Restore" }));
  await user.type(await screen.findByLabelText("Setup file"), FILE);
  await user.click(screen.getByRole("button", { name: "Check the file" }));
}

async function unlock(user: ReturnType<typeof userEvent.setup>) {
  await user.type(await screen.findByLabelText("Passphrase"), "correct passphrase");
  await user.click(screen.getByRole("button", { name: "Unlock secrets" }));
}

describe("SetupPage", () => {
  beforeEach(() => {
    resetPlusFixture();
    invokeLog.length = 0;
  });

  it("names every action it covers as built", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id]).toMatchObject({
        status: "built",
        test: "src/plus/setup/SetupPage.test.tsx",
      });
    }
  });

  describe("export", () => {
    async function ready() {
      const user = userEvent.setup();
      render(<SetupPage onBack={() => {}} />);
      await screen.findByLabelText("Repositories");
      return user;
    }

    it("test01_a_typed_passphrase_exports_no_secrets_while_secrets_stay_unticked", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByLabelText(/Secrets/));
      await user.type(screen.getByLabelText("Passphrase"), PASS);
      await user.click(screen.getByLabelText(/Secrets/));
      expect(screen.queryByLabelText("Passphrase")).not.toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Write the file" }));
      await waitFor(() => expect(lastExportArgv()).toBeDefined());
      const argv = lastExportArgv();
      expect(argv).not.toContain("--secrets");
      expect(argv).not.toContain("--passphrase-stdin");
      expect(argv[argv.indexOf("--parts") + 1]).not.toMatch(/secrets|history/);
      const sent = invokeLog
        .filter((e) => e.command === "plus_ctl")
        .map((e) => e.args as { stdinSecret: string | null });
      expect(sent.every((a) => !a.stdinSecret)).toBe(true);
    });

    it("test02_secrets_need_the_passphrase_twice_and_send_it_on_stdin_only", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByLabelText(/Secrets/));
      await user.type(screen.getByLabelText("Passphrase"), "short");
      expect(screen.getByRole("alert")).toHaveTextContent("shorter than 12");
      expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
      await user.clear(screen.getByLabelText("Passphrase"));
      await user.type(screen.getByLabelText("Passphrase"), PASS);
      await user.type(screen.getByLabelText("Repeat passphrase"), "another one entirely");
      expect(screen.getByRole("alert")).toHaveTextContent("differ");
      expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
      await user.clear(screen.getByLabelText("Repeat passphrase"));
      await user.type(screen.getByLabelText("Repeat passphrase"), PASS);
      await user.click(screen.getByRole("button", { name: "Write the file" }));
      await waitFor(() => expect(lastExportArgv()).toBeDefined());
      const argv = lastExportArgv();
      expect(argv).toContain("--secrets");
      expect(argv).toContain("--passphrase-stdin");
      expect(argv).not.toContain(PASS);
      const call = invokeLog
        .filter((e) => e.command === "plus_ctl")
        .map((e) => e.args as { argv: string[]; stdinSecret: string | null })
        .find((a) => a.argv[1] === "export")!;
      expect(call.stdinSecret).toBe(PASS);
    });

    it("test03_sessions_need_the_passphrase_but_never_switch_secrets_on", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByLabelText(/^Sessions/));
      expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
      await user.type(screen.getByLabelText("Passphrase"), PASS);
      await user.type(screen.getByLabelText("Repeat passphrase"), PASS);
      await user.click(screen.getByRole("button", { name: "Write the file" }));
      await waitFor(() => expect(lastExportArgv()).toBeDefined());
      expect(lastExportArgv()).toContain("--passphrase-stdin");
      expect(lastExportArgv()).not.toContain("--secrets");
    });

    it("test04_the_export_argv_takes_secrets_from_the_parts_alone", () => {
      const off = { passphrase: false, dryRun: false };
      expect(exportArgv(choice({ parts: ["repos"] }), "/o", off)).not.toContain(
        "--secrets",
      );
      const sessions = exportArgv(choice({ parts: ["repos", "history"] }), "/o", {
        passphrase: true,
        dryRun: false,
      });
      expect(sessions).toContain("--passphrase-stdin");
      expect(sessions).not.toContain("--secrets");
      expect(
        exportArgv(choice({ parts: ["secrets"] }), "/o", {
          passphrase: true,
          dryRun: true,
        }),
      ).toEqual([
        "setup",
        "export",
        "--parts",
        "secrets",
        "--out",
        "/o",
        "--secrets",
        "--passphrase-stdin",
        "--dry-run",
      ]);
    });

    it("test05_the_sub_choices_go_out_only_for_ticked_parts", () => {
      const argv = exportArgv(
        choice({
          parts: ["repos", "library", "claudeUser", "history"],
          without: ["claude-settings", "memory-auto"],
          roots: ["~/code", "~/work"],
          worktrees: false,
          skipNoRemote: true,
          libraryZip: true,
          sessionRepos: ["web"],
        }),
        "/o",
        { passphrase: true, dryRun: false },
      );
      expect(argv).toEqual([
        "setup",
        "export",
        "--parts",
        "repos,library,claudeUser,history",
        "--out",
        "/o",
        "--passphrase-stdin",
        "--roots",
        "~/code",
        "--roots",
        "~/work",
        "--no-worktrees",
        "--skip-no-remote",
        "--library-zip",
        "--without",
        "claude-settings",
        "--session-repo",
        "web",
      ]);
      const plain = exportArgv(
        choice({ parts: ["servers"], without: ["memory-auto"], worktrees: false }),
        "/o",
        { passphrase: false, dryRun: false },
      );
      expect(plain).not.toContain("--without");
      expect(plain).not.toContain("--no-worktrees");
    });

    it("test06_a_sub_choice_switched_off_reaches_the_command", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByLabelText(/settings\.json, secrets filtered/));
      await user.click(screen.getByLabelText(/Include worktrees/));
      await user.click(screen.getByLabelText(/Leave out repositories without a remote/));
      await user.click(screen.getByRole("button", { name: "Preview" }));
      await screen.findByText("Would write toolport-setup.zip");
      const argv = lastExportArgv();
      expect(argv[argv.indexOf("--without") + 1]).toBe("claude-settings");
      expect(argv).toContain("--no-worktrees");
      expect(argv).toContain("--skip-no-remote");
      expect(argv).toContain("--dry-run");
    });

    it("test07_source_folders_are_added_and_reach_the_listing_and_the_export", async () => {
      const user = await ready();
      expect(
        within(screen.getByRole("region", { name: "Source folders" })).getByText(
          "~/code",
        ),
      ).toBeInTheDocument();
      await user.type(screen.getByLabelText("Add a source folder"), "~/work");
      await user.click(screen.getByRole("button", { name: "Add folder" }));
      await waitFor(() =>
        expect(ranArgv().some((a) => a[1] === "parts" && a.includes("~/work"))).toBe(
          true,
        ),
      );
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByRole("button", { name: "Preview" }));
      await screen.findByText("Would write toolport-setup.zip");
      const argv = lastExportArgv();
      expect(argv[argv.indexOf("--roots") + 1]).toBe("~/work");
    });

    it("test08_sessions_are_chosen_per_repository", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByLabelText(/^Sessions/));
      await user.type(screen.getByLabelText("Passphrase"), PASS);
      await user.type(screen.getByLabelText("Repeat passphrase"), PASS);
      await user.click(screen.getByLabelText(/^api · 1 session/));
      await user.click(screen.getByRole("button", { name: "Preview" }));
      await screen.findByText("Would write toolport-setup.zip");
      const argv = lastExportArgv();
      expect(argv.filter((a) => a === "--session-repo")).toHaveLength(1);
      expect(argv[argv.indexOf("--session-repo") + 1]).toBe("web");
      await user.click(screen.getByLabelText(/^web · 2 sessions/));
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Choose at least one repository",
      );
      expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    });

    it("test09_files_the_secret_scan_held_back_are_listed_with_why_and_where_they_go", async () => {
      const user = await ready();
      const held = screen.getByRole("region", { name: "Held back by the secret scan" });
      expect(within(held).getByText("~/.claude/CLAUDE.md")).toBeInTheDocument();
      expect(held).toHaveTextContent("Looks like a GitHub token");
      expect(held).toHaveTextContent("stays out of the file");
      expect(held).toHaveTextContent("Tick Secrets");
      await user.click(screen.getByLabelText(/Secrets/));
      expect(
        screen.getByRole("region", { name: "Held back by the secret scan" }),
      ).toHaveTextContent("goes into the encrypted part");
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.type(screen.getByLabelText("Passphrase"), PASS);
      await user.type(screen.getByLabelText("Repeat passphrase"), PASS);
      await user.click(screen.getByRole("button", { name: "Preview" }));
      await screen.findByText("Would write toolport-setup.zip");
      const regions = screen.getAllByRole("region", {
        name: "Held back by the secret scan",
      });
      expect(regions.length).toBeGreaterThan(1);
      expect(regions.every((r) => /encrypted part/.test(r.textContent ?? ""))).toBe(true);
    });

    it("test10_a_part_can_be_peeked_at_before_the_file_is_written", async () => {
      const user = await ready();
      await user.click(screen.getByRole("button", { name: "Peek inside Repositories" }));
      expect(await screen.findByText("repos.json")).toBeInTheDocument();
      const argv = ranArgv().find((a) => a.includes("--peek"))!;
      expect(argv[argv.indexOf("--peek") + 1]).toBe("repos");
      expect(argv).toContain("--dry-run");
    });

    it("test11_a_written_file_is_reported_with_a_way_to_the_reset_checklist", async () => {
      const user = await ready();
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByRole("button", { name: "Write the file" }));
      expect(await screen.findByText("Export saved")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Open the reset checklist" }));
      expect(await screen.findByText("Not ready for a reset")).toBeInTheDocument();
    });
  });

  describe("restore", () => {
    it("test12_the_file_is_read_then_planned_and_nothing_is_applied_yet", async () => {
      const user = userEvent.setup();
      await openRestore(user);
      await unlock(user);
      expect(await screen.findByText("This is a dry run")).toBeInTheDocument();
      expect(screen.getByText(/Made on Studio Mac/)).toBeInTheDocument();
      expect(ranArgv().some((a) => a.includes("--apply"))).toBe(false);
      expect(ranArgv().some((a) => a[1] === "inspect")).toBe(true);
    });

    it("test13_a_file_with_an_encrypted_part_waits_for_the_passphrase_or_goes_without", async () => {
      const user = userEvent.setup();
      await openRestore(user);
      await screen.findByText("This file has an encrypted part");
      expect(ranArgv().some((a) => a[1] === "restore")).toBe(false);
      await user.type(screen.getByLabelText("Passphrase"), "not it");
      await user.click(screen.getByRole("button", { name: "Unlock secrets" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("does not open");
      await user.click(screen.getByRole("button", { name: "Continue without secrets" }));
      expect(await screen.findByText("This is a dry run")).toBeInTheDocument();
      const without = ranArgv()
        .filter((a) => a[1] === "restore")
        .at(-1)!;
      expect(without).not.toContain("--passphrase-stdin");
    });

    it("test14_a_plain_file_is_planned_at_once", async () => {
      resetPlusFixture({ setup: { plain: true } });
      const user = userEvent.setup();
      await openRestore(user);
      expect(await screen.findByText("This is a dry run")).toBeInTheDocument();
      expect(
        screen.queryByText("This file has an encrypted part"),
      ).not.toBeInTheDocument();
    });

    it("test15_a_conflict_is_kept_by_default_and_the_files_copy_goes_out_as_take", async () => {
      const user = userEvent.setup();
      await openRestore(user);
      await unlock(user);
      await screen.findByText("This is a dry run");
      expect(screen.getByLabelText("Keep mine: ~/.claude/CLAUDE.md")).toBeChecked();
      expect(screen.queryByLabelText(/Keep mine: web/)).not.toBeInTheDocument();
      expect(
        ranArgv()
          .filter((a) => a[1] === "restore")
          .at(-1),
      ).not.toContain("--take");
      await user.click(screen.getByLabelText("Use the file's: ~/.claude/CLAUDE.md"));
      await waitFor(() => {
        const argv = ranArgv()
          .filter((a) => a[1] === "restore")
          .at(-1)!;
        expect(argv[argv.indexOf("--take") + 1]).toBe("claude:CLAUDE.md");
      });
      await user.click(await screen.findByRole("button", { name: /^Restore/ }));
      expect(await screen.findByText("Setup restored")).toBeInTheDocument();
      const apply = ranArgv()
        .filter((a) => a.includes("--apply"))
        .at(-1)!;
      expect(apply).toContain("--take");
      expect(apply[apply.indexOf("--plan") + 1]).toBe("fixture-plan1");
    });

    it("test16_parts_are_chosen_and_only_those_are_planned", async () => {
      const user = userEvent.setup();
      await openRestore(user);
      await unlock(user);
      await screen.findByText("This is a dry run");
      expect(screen.getByLabelText("Sessions")).toBeDisabled();
      await user.click(
        within(screen.getByRole("region", { name: "Parts to restore" })).getByLabelText(
          "Servers",
        ),
      );
      await waitFor(() => {
        const argv = ranArgv()
          .filter((a) => a[1] === "restore")
          .at(-1)!;
        expect(argv[argv.indexOf("--parts") + 1].split(",")).not.toContain("servers");
      });
      expect(screen.queryByRole("region", { name: "Servers" })).not.toBeInTheDocument();
    });

    it("test17_a_failed_step_is_shown_and_can_be_planned_again", async () => {
      resetPlusFixture({ setup: { failures: true } });
      const user = userEvent.setup();
      await openRestore(user);
      await unlock(user);
      await user.click(await screen.findByRole("button", { name: /^Restore/ }));
      expect(
        await screen.findByText(/Restore finished, 1 step failed/),
      ).toBeInTheDocument();
      expect(screen.getByText("authentication required")).toBeInTheDocument();
      await user.click(
        screen.getByRole("button", { name: "Plan the failed steps again" }),
      );
      expect(await screen.findByText("This is a dry run")).toBeInTheDocument();
    });

    it("test18_undo_shows_what_it_would_do_then_reports_done_kept_and_failed_per_step", async () => {
      resetPlusFixture({ setup: { failures: true } });
      const user = userEvent.setup();
      await openRestore(user);
      await unlock(user);
      await user.click(await screen.findByRole("button", { name: /^Restore/ }));
      await screen.findByText(/Restore finished/);
      await user.click(screen.getByRole("button", { name: "Undo this restore" }));
      const steps = await screen.findByRole("list", { name: "Undo steps" });
      expect(steps).toHaveTextContent("clone removed");
      expect(
        ranArgv()
          .filter((a) => a[1] === "undo")
          .at(-1),
      ).toContain("--dry-run");
      await user.click(screen.getByRole("button", { name: "Undo" }));
      expect(
        await screen.findByText(/Not everything was undone: 1 step failed/),
      ).toBeInTheDocument();
      const after = screen.getByRole("list", { name: "Undo steps" });
      expect(after).toHaveTextContent("Put back");
      expect(after).toHaveTextContent("Kept");
      expect(after).toHaveTextContent("Could not");
      expect(after).toHaveTextContent("read-only");
      await user.click(
        screen.getByRole("button", { name: /Try the failed steps again/ }),
      );
      expect(await screen.findByText("Restore undone")).toBeInTheDocument();
      const retry = ranArgv()
        .filter((a) => a[1] === "undo")
        .at(-1)!;
      expect(retry).toContain("run-1");
      expect(retry).not.toContain("--dry-run");
    });

    it("test19_restore_argv_carries_take_parts_and_the_plan", () => {
      expect(
        restoreArgv(FILE, {
          sealed: true,
          take: ["a", "b"],
          parts: ["repos", "servers"],
          plan: "h",
        }),
      ).toEqual([
        "setup",
        "restore",
        FILE,
        "--passphrase-stdin",
        "--parts",
        "repos,servers",
        "--take",
        "a",
        "--take",
        "b",
        "--apply",
        "--plan",
        "h",
      ]);
      expect(restoreArgv(FILE, { sealed: false })).toEqual(["setup", "restore", FILE]);
    });
  });

  describe("before a reset", () => {
    async function openReset() {
      const user = userEvent.setup();
      render(<SetupPage onBack={() => {}} />);
      await user.click(screen.getByRole("tab", { name: "Before a reset" }));
      await screen.findByText("Not ready for a reset");
      return user;
    }

    it("ticks what you carry over and acknowledges an amber finding", async () => {
      const user = await openReset();
      expect(screen.queryByRole("button", { name: "Acknowledge" })).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Acknowledge" }));
      await screen.findByText("Acknowledged");
      await user.click(await screen.findByLabelText("browser"));
      await waitFor(() => expect(screen.getByLabelText("browser")).toBeChecked());
    });

    it("test20_push_commands_are_shown_to_copy_and_never_run", async () => {
      const user = await openReset();
      const write = vi.spyOn(navigator.clipboard, "writeText");
      expect(screen.getByText("git -C ~/code/web push origin main")).toBeInTheDocument();
      await user.click(
        screen.getByRole("button", {
          name: "Copy the commands for ~/code/web, unpushed",
        }),
      );
      expect(write).toHaveBeenCalledWith("git -C ~/code/web push origin main");
      expect(ranArgv().every((a) => a[0] === "setup")).toBe(true);
      expect(ranArgv().some((a) => a.includes("push"))).toBe(false);
    });

    it("test21_a_blocker_that_only_a_push_fixes_cannot_be_acknowledged", async () => {
      await openReset();
      const row = screen.getByText("1 commits are on no remote").closest("li")!;
      expect(
        within(row).queryByRole("button", { name: "Acknowledge" }),
      ).not.toBeInTheDocument();
    });

    it("test22_without_an_export_the_checklist_offers_the_first_one", async () => {
      const user = await openReset();
      expect(screen.getByText("No export yet")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "Make the first export" }));
      expect(await screen.findByLabelText("Write to")).toBeInTheDocument();
    });

    it("test23_after_an_export_the_age_what_it_covers_and_what_changed_since_show", async () => {
      const user = userEvent.setup();
      render(<SetupPage onBack={() => {}} />);
      await screen.findByLabelText("Write to");
      await user.type(screen.getByLabelText("Write to"), "/tmp/out");
      await user.click(screen.getByRole("button", { name: "Write the file" }));
      await screen.findByText("Export saved");
      await user.click(screen.getByRole("button", { name: "Open the reset checklist" }));
      const covered = await screen.findByRole("region", {
        name: "Covered by the export",
      });
      expect(covered).toHaveTextContent(/\d+ days old|today/);
      expect(
        within(covered).getByRole("list", { name: "What the export covers" }),
      ).toHaveTextContent("Repositories");
      await user.click(
        screen.getByRole("button", { name: "Check what changed since the export" }),
      );
      await waitFor(() =>
        expect(
          ranArgv()
            .filter((a) => a[1] === "check")
            .at(-1),
        ).toContain("--since"),
      );
      expect(await screen.findByText(/1 repository changed branch/)).toBeInTheDocument();
    });

    it("test24_sessions_are_listed_with_a_way_to_choose_them_for_the_export", async () => {
      const user = await openReset();
      const sessions = screen.getByRole("region", { name: "Sessions" });
      expect(sessions).toHaveTextContent("3 sessions in 2 repositories");
      await user.click(
        within(sessions).getByRole("button", { name: "Choose sessions to export" }),
      );
      expect(await screen.findByLabelText(/^Sessions/)).toBeChecked();
      expect(screen.getByLabelText("Passphrase")).toBeInTheDocument();
    });

    it("test25_the_since_line_names_branches_and_files", () => {
      expect(sinceLine([])).toBe("Nothing changed since.");
      expect(
        sinceLine([
          {
            id: "repos",
            label: "Repositories",
            added: 0,
            removed: 0,
            changed: 1,
            countWas: 1,
            countNow: 1,
            branchChanges: 3,
          },
          {
            id: "memory",
            label: "Memory",
            added: 1,
            removed: 0,
            changed: 2,
            countWas: 1,
            countNow: 1,
          },
        ]),
      ).toBe("Since then: 3 repositories changed branch, Memory: 3 files changed.");
    });
  });
});
