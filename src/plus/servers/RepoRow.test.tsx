import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { RepoSeed } from "../fixtures/repoWorld";
import { fixtureServers } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { AttentionBanner } from "../attention/AttentionBanner";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { PlusFooterBell } from "../shell/PlusFooterBellSlot";
import ServerOperations from "./ServerOperations";
import { resetRestartStore } from "./restartStore";
import { entryOf, getRegistryMock, invokeLog, ranArgv, registryOf } from "./testkit";
import { resetServerWatch } from "./watch";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("./testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api")>()),
  getRegistry: (...args: unknown[]) => getRegistryMock(...args),
}));

const servers = fixtureServers();
const sets = [
  { id: "local", name: "Local fixture", serverIds: servers.map((s) => s.id) },
];
const registry = registryOf(servers, sets);
const host = { openServer: vi.fn(), openAccessSets: vi.fn(), closeServer: vi.fn() };

const BEHIND: RepoSeed = {
  behind: 2,
  commits: [
    { id: "9be07d2b", subject: "Fix pagination off-by-one" },
    { id: "c41e0aab", subject: "Add list_projects tool" },
  ],
  files: [
    { status: "M", path: "src/paging.ts" },
    { status: "A", path: "src/projects.ts" },
  ],
};

function start(seed?: RepoSeed, serverIndex = 0) {
  resetPlusFixture({
    servers: { servers: fixtureServers(), repos: seed ? { "fixture-0": seed } : {} },
  });
  render(
    <ServerOperations
      server={entryOf(servers[serverIndex])}
      registry={registry}
      host={host}
      onRegistryChange={vi.fn()}
    />,
  );
}

const repoArgv = () => ranArgv().filter((argv) => argv[1] === "repo");

beforeEach(() => {
  getRegistryMock.mockReset().mockResolvedValue(registry);
  invokeLog.length = 0;
  resetRestartStore();
  resetServerWatch();
  registerAttentionFeed(null);
  vi.clearAllMocks();
});

describe("servers.repo-status and servers.repo-update", () => {
  it("test00_both_repo_actions_are_built_and_name_this_file", () => {
    for (const action of ["servers.repo-status", "servers.repo-update"]) {
      expect(guiParity.actions[action]).toMatchObject({
        status: "built",
        test: "src/plus/servers/RepoRow.test.tsx",
      });
    }
  });

  it("test01_a_server_that_runs_from_no_checkout_has_no_repo_row", async () => {
    start();
    await screen.findByRole("button", { name: /^Runtime/ });
    await waitFor(() => expect(repoArgv().length).toBeGreaterThan(0));
    expect(screen.queryByText("Repo")).not.toBeInTheDocument();
  });

  it("test02_a_checkout_behind_its_branch_says_so_and_check_now_fetches", async () => {
    start(BEHIND);
    expect(
      await screen.findByText(/main → origin\/main · 2 new commits/),
    ).toBeInTheDocument();
    expect(screen.getByText("Update available")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Check now" }));
    await waitFor(() => expect(repoArgv()).toContainEqual(["server", "repo", "check"]));
  });

  it("test03_local_changes_and_a_diverged_branch_show_a_calm_line_and_no_update_button", async () => {
    start({ ...BEHIND, dirty: 2 });
    expect(await screen.findByText(/local changes block the update/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Review update" }),
    ).not.toBeInTheDocument();
  });

  it("test04_a_diverged_branch_is_left_to_the_person", async () => {
    start({ ...BEHIND, ahead: 1 });
    expect(
      await screen.findByText(/diverged: 1 local and 2 new commits/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Review update" }),
    ).not.toBeInTheDocument();
  });

  it("test05_the_plan_lists_commits_and_files_and_the_update_runs_with_the_plan_hash", async () => {
    start(BEHIND);
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("Fix pagination off-by-one"),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("src/projects.ts")).toBeInTheDocument();
    expect(within(dialog).getByText("2 files change")).toBeInTheDocument();
    expect(within(dialog).getByRole("checkbox")).not.toBeChecked();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    expect(
      await within(dialog).findByText(/Updated GitHub: 4f2a1c3a to 9be07d2b/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText(/Restarted GitHub/)).toBeInTheDocument();
    const apply = repoArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply.slice(0, 4)).toEqual(["server", "repo", "update", "GitHub"]);
    expect(apply).toContain("--no-build");
  });

  it("test06_a_remembered_build_is_ticked_and_runs_without_asking_for_the_text_again", async () => {
    start({ ...BEHIND, remembered: "./build.sh" });
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByRole("checkbox")).toBeChecked();
    expect(within(dialog).getByLabelText("Build command")).toHaveValue("./build.sh");
    expect(within(dialog).getByText(/agreed to this command/)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    expect(await within(dialog).findByText(/Build finished/)).toBeInTheDocument();
    const apply = repoArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply).not.toContain("--build");
    expect(apply).not.toContain("--no-build");
  });

  it("test12_a_remembered_command_with_a_secret_is_not_taken_and_a_typed_one_is_refused", async () => {
    start({
      ...BEHIND,
      remembered: "make --token=fake-secret-value-123",
      blocked: "the remembered build command was not taken: it carries a secret",
    });
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/was not taken/)).toBeInTheDocument();
    expect(within(dialog).getByRole("checkbox")).not.toBeChecked();
    expect(dialog.textContent).not.toContain("fake-secret-value-123");
    await userEvent.click(within(dialog).getByRole("checkbox"));
    await userEvent.type(within(dialog).getByLabelText("Build command"), "make token=x");
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    expect(await within(dialog).findByText(/carries a secret/)).toBeInTheDocument();
    expect(repoArgv().some((argv) => argv.includes("--plan-hash"))).toBe(false);
  });

  it("test07_a_different_build_command_is_passed_explicitly_as_a_new_yes", async () => {
    start({ ...BEHIND, remembered: "./build.sh" });
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    const field = await within(dialog).findByLabelText("Build command");
    await userEvent.clear(field);
    await userEvent.type(field, "make");
    expect(
      within(dialog).getByText(/A different command is a new yes/),
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    await within(dialog).findByText(/Build finished/);
    const apply = repoArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply).toEqual(expect.arrayContaining(["--build", "make"]));
  });

  it("test08_a_failed_build_shows_its_output_and_does_not_restart_and_undo_moves_back", async () => {
    start({ ...BEHIND, remembered: "./build.sh", buildFails: true });
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    await within(dialog).findByLabelText("Build command");
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    expect(await within(dialog).findByText(/Build failed/)).toBeInTheDocument();
    expect(within(dialog).getByText(/could not compile alpha-api/)).toBeInTheDocument();
    expect(within(dialog).getByText(/server was not restarted/)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo" }));
    expect(
      await within(dialog).findByText(/Moved back GitHub: 9be07d2b to 4f2a1c3a/),
    ).toBeInTheDocument();
    expect(repoArgv().filter((argv) => argv[2] === "undo")).toEqual([
      ["server", "repo", "undo", "GitHub"],
    ]);
  });

  it("test09_a_running_call_keeps_the_restart_back_until_restart_now_is_pressed", async () => {
    start({ ...BEHIND, inFlight: 1 });
    await userEvent.click(await screen.findByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/1 call running now/)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Update GitHub" }));
    expect(
      await within(dialog).findByText(/A call is still running/),
    ).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Restart now" }));
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["server", "restart", "GitHub"]),
    );
  });

  it("test10_a_refused_plan_is_explained_and_cannot_be_applied", async () => {
    resetPlusFixture({
      servers: {
        servers: fixtureServers(),
        repos: { "fixture-0": { ...BEHIND, dirty: 3 } },
      },
    });
    const { default: Dialog } = await import("./RepoUpdateDialog");
    render(<Dialog serverName="GitHub" open onOpenChange={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /local changes in 3 tracked file\(s\) block the update/,
    );
    expect(screen.getByRole("button", { name: "Update GitHub" })).toBeDisabled();
  });
});

describe("the attention row of a server update", () => {
  it("test11_the_servers_banner_opens_the_plan_instead_of_running_a_command", async () => {
    const list = fixtureServers();
    list[0].name = "alpha-api";
    resetPlusFixture({
      attention: { scenario: "server-update" },
      servers: { servers: list, repos: { "fixture-0": BEHIND } },
    });
    render(
      <>
        <PlusFooterBell />
        <AttentionBanner page="servers" />
      </>,
    );
    expect(
      await screen.findByText("alpha-api: 2 new commits on main"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Review update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Add list_projects tool")).toBeInTheDocument();
    expect(repoArgv().every((argv) => argv.includes("--dry-run"))).toBe(true);
  });
});
