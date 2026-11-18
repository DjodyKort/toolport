import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { WorldOptions } from "../fixtures/ctlWorld";
import { guiParity } from "../guiParity";
import { SetupPage } from "./SetupPage";
import SyncView from "./SyncView";

const calls: { argv: string[]; stdin: string | null }[] = [];

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      if (command === "plus_ctl")
        calls.push({
          argv: args.argv as string[],
          stdin: (args.stdinSecret as string | null) ?? null,
        });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

/** The actions of src/plus/gui-parity/ this file proves. */
const COVERS = [
  "sync.status",
  "sync.setup",
  "sync.pull",
  "sync.push",
  "sync.undo",
  "sync.repos",
] as const;

const PASSPHRASE = "correct horse battery";

const syncCalls = (which: { repos: boolean } = { repos: false }) =>
  calls
    .filter((c) => c.argv[0] === "sync")
    .map((c) => c.argv.join(" "))
    .filter((argv) => which.repos || argv !== "sync repos");

function world(sync: NonNullable<WorldOptions["sync"]> = {}) {
  resetPlusFixture({ sync });
}

describe("SyncView", () => {
  beforeEach(() => {
    calls.length = 0;
    world();
  });

  it("test01_names_every_action_it_covers_as_built", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id]).toMatchObject({
        status: "built",
        route: "setup",
        test: "src/plus/setup/SyncView.test.tsx",
      });
    }
  });

  it("test02_opens_from_the_sync_tab_of_setup_and_backup", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await user.click(screen.getByRole("tab", { name: "Sync" }));
    expect(await screen.findByText("Encrypted sync")).toBeInTheDocument();
  });

  it("test03_shows_the_status_card_machines_and_what_syncs_with_counts", async () => {
    render(<SyncView />);
    expect(await screen.findByText("Encrypted sync")).toBeInTheDocument();
    expect(screen.getByText("On")).toBeInTheDocument();
    expect(
      screen.getByText(/git@example.invalid:me\/toolport-sync.git/),
    ).toHaveTextContent(/branch main · last synced 4 min ago/);
    const machines = within(screen.getByRole("list", { name: "Machines" }));
    expect(machines.getByText("This machine · Studio")).toBeInTheDocument();
    expect(machines.getByText("Laptop · 3 h ago")).toBeInTheDocument();
    expect(screen.getByText("2 servers")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Sync Servers" })).toBeChecked();
    expect(screen.getByRole("switch", { name: "Sync Secrets" })).not.toBeChecked();
  });

  it("test04_a_part_switch_rewrites_the_list_of_parts_that_sync", async () => {
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(await screen.findByRole("switch", { name: "Sync Memory" }));
    await waitFor(() =>
      expect(syncCalls()).toContain(
        "sync setup --parts servers,access,tasks,settings,memory,serverRepos",
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Sync Memory" })).toBeChecked(),
    );
  });

  it("test05_sync_now_shows_the_plan_first_then_applies_it_with_the_hash_and_pushes", async () => {
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(await screen.findByRole("button", { name: /Sync now/ }));
    const plan = await screen.findByRole("region", { name: "Plan" });
    expect(within(plan).getByText("alpha")).toBeInTheDocument();
    expect(within(plan).getByText("Changed on the other machine")).toBeInTheDocument();
    expect(syncCalls()).toEqual(["sync status", "sync pull"]);
    await user.click(within(plan).getByRole("button", { name: "Apply 1 changes" }));
    await screen.findByText("1 server arrived from Laptop");
    await waitFor(() => expect(syncCalls()).toContain("sync push"));
    const order = syncCalls();
    expect(order.indexOf("sync pull --apply --plan fixture-sync-plan")).toBeGreaterThan(
      order.indexOf("sync pull"),
    );
    expect(order.indexOf("sync push")).toBeGreaterThan(
      order.indexOf("sync pull --apply --plan fixture-sync-plan"),
    );
  });

  it("test06_a_conflict_row_offers_this_machine_and_other_machine", async () => {
    world({ conflicts: ["server:calendar"], remote: "same" });
    const user = userEvent.setup();
    render(<SyncView />);
    const needs = await screen.findByRole("region", { name: "Needs you" });
    expect(
      within(needs).getByText("calendar changed on two machines"),
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(within(needs).getByText(/yours is kept until you take/)).toBeInTheDocument(),
    );
    await user.click(
      within(needs).getByRole("button", { name: "Other machine for calendar" }),
    );
    const plan = await screen.findByRole("region", { name: "Plan" });
    expect(within(plan).getByText("Other machine's copy")).toBeInTheDocument();
    expect(syncCalls()).toContain("sync pull --take server:calendar");
    expect(syncCalls()).not.toContain(
      "sync pull --take server:calendar --apply --plan fixture-sync-plan",
    );
    await user.click(within(plan).getByRole("button", { name: /Apply/ }));
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Needs you" })).toBeNull(),
    );
    expect(syncCalls()).toContain(
      "sync pull --take server:calendar --apply --plan fixture-sync-plan",
    );
  });

  it("test07_keeping_this_machine_settles_the_row_without_changing_a_file", async () => {
    world({ conflicts: ["server:calendar"], remote: "same" });
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(
      await screen.findByRole("button", { name: "This machine for calendar" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Needs you" })).toBeNull(),
    );
    expect(syncCalls()).toContain(
      "sync pull --apply --plan fixture-sync-plan --keep server:calendar",
    );
  });

  it("test08_a_push_is_refused_while_a_conflict_waits_and_says_which", async () => {
    world({ conflicts: ["server:calendar"], remote: "same" });
    const user = userEvent.setup();
    render(<SyncView />);
    await screen.findByRole("region", { name: "Needs you" });
    await user.click(screen.getByRole("button", { name: /Sync now/ }));
    await waitFor(() => expect(syncCalls()).toContain("sync push"));
    expect(screen.getByRole("region", { name: "Needs you" })).toBeInTheDocument();
  });

  it("test09_undo_puts_the_last_apply_back_and_offers_the_change_again", async () => {
    world({ remote: "ahead" });
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(await screen.findByRole("button", { name: /Sync now/ }));
    await user.click(await screen.findByRole("button", { name: /Apply/ }));
    const undo = await screen.findByRole("button", { name: /Undo the last sync/ });
    await user.click(undo);
    await waitFor(() => expect(syncCalls()).toContain("sync undo"));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Undo the last sync/ })).toBeNull(),
    );
    await user.click(screen.getByRole("button", { name: /Sync now/ }));
    expect(await screen.findByText("Changed on the other machine")).toBeInTheDocument();
  });

  it("test10_a_locked_keychain_shows_paused_and_sync_now_asks_once", async () => {
    world({ locked: true });
    const user = userEvent.setup();
    render(<SyncView />);
    expect(
      await screen.findByText(/Sync is paused: the keychain is locked/),
    ).toBeInTheDocument();
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByText(/last synced 4 min ago/)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Machines" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Sync now" }));
    await waitFor(() => expect(syncCalls()).toContain("sync pull"));
    expect(syncCalls().join("\n")).not.toContain("--unattended");
  });

  it("test11_first_run_asks_for_the_passphrase_twice_and_sends_it_on_stdin_only", async () => {
    world({ configured: false });
    const user = userEvent.setup();
    render(<SyncView />);
    await screen.findByText("Where to keep it");
    const start = screen.getByRole("button", { name: "Preview the first sync" });
    expect(start).toBeDisabled();
    await user.type(
      screen.getByLabelText("Repository URL"),
      "git@example.invalid:me/s.git",
    );
    await user.type(screen.getByLabelText("Passphrase"), "short");
    expect(screen.getByRole("alert")).toHaveTextContent("shorter than 12");
    await user.clear(screen.getByLabelText("Passphrase"));
    await user.type(screen.getByLabelText("Passphrase"), PASSPHRASE);
    await user.type(screen.getByLabelText("Repeat the passphrase"), "something else");
    expect(screen.getByRole("alert")).toHaveTextContent("differ");
    expect(start).toBeDisabled();
    await user.clear(screen.getByLabelText("Repeat the passphrase"));
    await user.type(screen.getByLabelText("Repeat the passphrase"), PASSPHRASE);
    expect(screen.getByText("Matches")).toBeInTheDocument();
    await user.click(start);
    expect(await screen.findByRole("region", { name: "Plan" })).toBeInTheDocument();
    const setup = calls.find((c) => c.argv[1] === "setup")!;
    expect(setup.stdin).toBe(PASSPHRASE);
    expect(setup.argv).toContain("--passphrase-stdin");
    expect(calls.flatMap((c) => c.argv).join(" ")).not.toContain(PASSPHRASE);
    expect(calls.map((c) => c.argv.join(" "))).toContain("sync pull");
  });

  it("test12_the_hosted_service_is_listed_but_cannot_be_chosen", async () => {
    world({ configured: false });
    render(<SyncView />);
    const hosted = await screen.findByRole("radio", { name: /hosted sync/ });
    expect(hosted).toBeDisabled();
    expect(screen.getByText("Not available here")).toBeInTheDocument();
  });

  it("test13_an_empty_remote_previews_the_first_push_before_anything_leaves", async () => {
    world({ remote: "empty" });
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(await screen.findByRole("button", { name: /Sync now/ }));
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("Would push 2 parts");
    expect(status).toHaveTextContent("your code and anything uncommitted");
    expect(syncCalls()).toContain("sync push --dry-run");
    expect(syncCalls()).not.toContain("sync push");
    await user.click(within(status).getByRole("button", { name: "Push now" }));
    await waitFor(() => expect(syncCalls()).toContain("sync push"));
  });

  it("test14_show_what_would_be_pushed_runs_a_dry_run", async () => {
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(
      await screen.findByRole("button", { name: "Show what would be pushed" }),
    );
    expect(await screen.findByText(/Would push 2 parts/)).toBeInTheDocument();
    expect(syncCalls()).not.toContain("sync push");
  });

  it("test15_upstream_sync_owning_servers_leaves_that_row_without_a_switch", async () => {
    world({ upstream: true });
    render(<SyncView />);
    expect(
      await screen.findByText("Upstream Sync (Pro) is on and handles Servers."),
    ).toBeInTheDocument();
    expect(screen.getByText("Handled by upstream Sync")).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "Sync Servers" })).toBeNull();
    expect(screen.getByRole("switch", { name: "Sync Tasks" })).toBeInTheDocument();
  });

  it("test16_settings_save_the_schedule_and_leave_a_stored_passphrase_alone", async () => {
    const user = userEvent.setup();
    render(<SyncView />);
    await user.click(await screen.findByRole("button", { name: "Settings…" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(
      within(dialog).getByRole("switch", { name: "Check every 30 minutes" }),
    );
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(syncCalls().some((c) => c.includes("--schedule on"))).toBe(true),
    );
    const save = calls.find((c) => c.argv.includes("--schedule"))!;
    expect(save.stdin).toBeNull();
    expect(save.argv).not.toContain("--passphrase-stdin");
  });

  it("test17_a_status_that_cannot_be_read_offers_a_retry", async () => {
    world({ statusError: "toolportctl said no" });
    render(<SyncView />);
    expect(
      await screen.findByText("Could not read the sync settings"),
    ).toBeInTheDocument();
    expect(screen.getByText("toolportctl said no")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });

  it("test18_on_a_narrow_pane_the_choices_wrap_under_their_row", async () => {
    world({ conflicts: ["server:calendar"] });
    render(<SyncView />);
    const needs = await screen.findByRole("region", { name: "Needs you" });
    const row = within(needs)
      .getByText("calendar changed on two machines")
      .closest("li")!;
    expect(row.className).toContain("@max-[560px]:flex-col");
    const actions = row.querySelector('[data-slot="item-actions"]')!;
    expect(
      within(actions as HTMLElement).getByRole("button", { name: /This machine/ }),
    ).toBeInTheDocument();
    expect(
      within(actions as HTMLElement).getByRole("button", { name: /Other machine/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("region", { name: "What syncs" }).closest(".\\@container"),
    ).not.toBeNull();
  });
  it("test19_the_repos_list_shows_this_machine_and_the_other_machines_checkouts", async () => {
    render(<SyncView />);
    const repos = await screen.findByRole("region", { name: "Repos" });
    expect(
      await within(repos).findByText(/2 commits on no remote, so Laptop cannot see them/),
    ).toBeInTheDocument();
    expect(within(repos).getByText(/Laptop · feature at 77aa11bb/)).toBeInTheDocument();
    expect(within(repos).getByText("other branch")).toBeInTheDocument();
    expect(syncCalls({ repos: true })).toContain("sync repos");
  });

  it("test20_the_repos_list_explains_itself_when_the_part_is_switched_off", async () => {
    world({});
    const user = userEvent.setup();
    render(<SyncView />);
    await screen.findByRole("region", { name: "Repos" });
    await user.click(
      await screen.findByRole("switch", { name: "Sync Server repositories" }),
    );
    expect(
      await screen.findByText(/does not publish its server checkouts/),
    ).toBeInTheDocument();
  });
});
