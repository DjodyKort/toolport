import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { PackageSeed } from "../fixtures/packageWorld";
import type { RestartScript } from "../fixtures/serverWorld";
import { fixtureServers } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { AttentionBanner } from "../attention/AttentionBanner";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { PlusFooterBell } from "../shell/PlusFooterBellSlot";
import { ranArgv, invokeLog } from "./testkit";
import PackageUpdateDialog from "./PackageUpdateDialog";

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

const NPM: PackageSeed = {
  kind: "npm",
  package: "@scope/alpha",
  from: "1.4.2",
  to: "1.5.0",
};
const RELEASE: PackageSeed = {
  kind: "github",
  package: "acme/alpha",
  from: "1.3.0",
  to: "1.4.0",
  release: {
    asset: "alpha_1.4.0_darwin_arm64.tar.gz",
    size: 12_400_000,
    checksum: "checksums.txt",
    file: "/Users/example/.claude/mcp-servers/alpha/alpha",
  },
};

function start(
  seed: PackageSeed,
  scenario?: "package-update" | "release-update" | "release-manual",
  restart?: RestartScript,
) {
  const list = fixtureServers();
  list[0].name = "alpha-api";
  resetPlusFixture({
    servers: {
      servers: list,
      packages: { "fixture-0": seed },
      restart: restart ? { "fixture-0": restart } : undefined,
    },
    ...(scenario && { attention: { scenario } }),
  });
}

const updateArgv = () =>
  ranArgv().filter((argv) => argv[0] === "server" && argv[1] === "update");

function open() {
  render(<PackageUpdateDialog serverName="alpha-api" open onOpenChange={vi.fn()} />);
}

beforeEach(() => {
  invokeLog.length = 0;
  registerAttentionFeed(null);
  vi.clearAllMocks();
});

describe("servers.package-update", () => {
  it("test00_the_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["servers.package-update"]).toMatchObject({
      status: "built",
      test: "src/plus/servers/PackageUpdateDialog.test.tsx",
    });
    expect(guiParity.commands["server update"].action).toBe("servers.package-update");
    expect(guiParity.tools.servers_update.action).toBe("servers.package-update");
  });

  it("test01_the_plan_shows_the_exact_registry_change_and_asks_the_registry_first", async () => {
    start(NPM);
    open();
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Review updates")).toBeInTheDocument();
    expect(await within(dialog).findByLabelText("Update alpha-api")).toBeChecked();
    expect(
      await within(dialog).findByText(/Checked \d\d:\d\d.*\. Only the ticked/),
    ).toBeVisible();
    expect(within(dialog).getByText("npm")).toBeInTheDocument();
    expect(within(dialog).getByText("1.4.2 to 1.5.0")).toBeInTheDocument();
    expect(within(dialog).getByText("args[1]")).toBeInTheDocument();
    expect(within(dialog).getByText("@scope/alpha@1.4.2")).toBeInTheDocument();
    expect(within(dialog).getByText("@scope/alpha@1.5.0")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/Does not restart anything. You will be asked after/),
    ).toBeInTheDocument();
    const plan = updateArgv()[0];
    expect(plan).toEqual(
      expect.arrayContaining(["server", "update", "alpha-api", "--dry-run", "--fetch"]),
    );
    expect(updateArgv().filter((argv) => !argv.includes("--dry-run"))).toHaveLength(0);
  });

  it("test02_confirming_applies_the_reviewed_plan_without_a_restart_and_undo_puts_it_back", async () => {
    start(NPM);
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    expect(await within(dialog).findByText("alpha-api updated")).toBeInTheDocument();
    expect(within(dialog).getByText(/updated to 1.5.0/)).toBeInTheDocument();
    expect(
      within(dialog).getByText(/alpha-api still run the old version until restarted/),
    ).toBeInTheDocument();
    const apply = updateArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply).toEqual([
      "server",
      "update",
      "alpha-api",
      "--to",
      "1.5.0",
      "--plan-hash",
      "sha256:1.4.2>1.5.0",
      "--no-restart",
    ]);
    expect(ranArgv().filter((argv) => argv[1] === "restart")).toHaveLength(0);
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo" }));
    expect(await within(dialog).findByText("alpha-api put back")).toBeInTheDocument();
    expect(within(dialog).getByText(/put back to 1.4.2/)).toBeInTheDocument();
    expect(updateArgv()).toContainEqual([
      "server",
      "update",
      "alpha-api",
      "--undo",
      "--no-restart",
    ]);
    expect(within(dialog).queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("test03_a_release_without_a_checksum_waits_for_the_person_to_allow_it", async () => {
    start({ ...RELEASE, release: { ...RELEASE.release!, checksum: null } });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(
      (await within(dialog).findAllByText("alpha_1.4.0_darwin_arm64.tar.gz")).length,
    ).toBeGreaterThan(0);
    expect(within(dialog).getByText("no checksum published")).toBeInTheDocument();
    expect(within(dialog).getByRole("note")).toHaveTextContent(
      /cannot be verified. It stays unticked until you allow it/,
    );
    const server = within(dialog).getByLabelText("Update alpha-api");
    const apply = within(dialog).getByRole("button", { name: "Update 0 servers" });
    expect(server).not.toBeChecked();
    expect(server).toBeDisabled();
    expect(apply).toBeDisabled();
    await userEvent.click(
      within(dialog).getByRole("checkbox", { name: /Install without verification/ }),
    );
    expect(server).toBeChecked();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Update alpha-api" }),
    );
    await within(dialog).findByText("alpha-api updated");
    expect(updateArgv().find((argv) => argv.includes("--plan-hash"))).toContain(
      "--allow-unverified",
    );
  });

  it("test04_a_release_with_a_checksum_names_the_file_and_what_checks_it", async () => {
    start(RELEASE);
    open();
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Checked against checksums.txt \(sha256\)/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("GitHub release")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/1.3.0 to 1.4.0 · replaces the binary in place/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("mcpmSource.current_version")).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("checkbox", { name: /Install without verification/ }),
    ).toBeNull();
    expect(within(dialog).getByText(/12 MB/)).toBeInTheDocument();
  });

  it("test05_an_image_digest_says_it_is_pinned_and_pulled_on_the_next_start", async () => {
    start({
      kind: "docker",
      package: "acme/files",
      tracks: "latest",
      from: `sha256:${"a".repeat(64)}`,
      to: `sha256:${"b".repeat(64)}`,
    });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("Docker image")).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        /Digest sha256:aaaaaaaaaaaa\.\.\. to sha256:bbbbbbbbbbbb\.\.\./,
      ),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/The image is pulled on the next start/),
    ).toBeInTheDocument();
  });

  it("test06_a_plan_that_cannot_be_applied_says_why_and_offers_no_update", async () => {
    start({
      ...NPM,
      refusal: {
        code: "cannot_rewrite",
        message:
          "Toolport cannot change this pin itself: its arguments come from a catalog launch recipe",
      },
    });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/launch recipe/)).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Update alpha-api")).toBeDisabled();
    expect(
      within(dialog).getByRole("button", { name: "Update 0 servers" }),
    ).toBeDisabled();
  });

  it("test07_a_server_already_on_the_newest_version_says_nothing_is_to_do", async () => {
    start({ ...NPM, to: "1.4.2" });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(
        /Nothing to update: @scope\/alpha is already on 1.4.2/,
      ),
    ).toBeInTheDocument();
  });

  it("test08_an_unreachable_registry_shows_its_reason_and_changes_nothing", async () => {
    start({ ...NPM, offline: "registry.npmjs.org: could not connect (no route)" });
    open();
    expect(await screen.findByRole("alert")).toHaveTextContent(/no route/);
    expect(updateArgv().filter((argv) => !argv.includes("--dry-run"))).toHaveLength(0);
  });

  it("test09_a_plan_that_changed_since_the_review_is_shown_again_instead_of_applied", async () => {
    start({ ...NPM, conflictOnce: "1.4.5" });
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    expect(
      await within(dialog).findByText(/The plan changed while this was open/),
    ).toBeInTheDocument();
    expect(await within(dialog).findByText("@scope/alpha@1.4.5")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Update alpha-api")).toBeChecked();
  });

  it("test10_the_restart_the_result_asks_for_waits_for_a_running_call_until_the_person_insists", async () => {
    start(NPM, undefined, { kind: "busy" });
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Restart 1" }),
    );
    expect(ranArgv()).toContainEqual(["server", "restart", "alpha-api", "--if-idle"]);
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Restart now" }),
    );
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["server", "restart", "alpha-api"]),
    );
    expect(await within(dialog).findByText("Restarted")).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: /^Restart \d/ })).toBeNull();
  });

  it("test11_a_restart_of_an_idle_server_ends_the_prompt", async () => {
    start(NPM);
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Restart 1" }),
    );
    expect(await within(dialog).findByText("Restarted")).toBeInTheDocument();
    expect(within(dialog).queryByText(/still run the old version/)).toBeNull();
    expect(within(dialog).queryByRole("button", { name: /^Restart \d/ })).toBeNull();
  });

  it("test12_a_failed_apply_names_the_reason_and_leaves_the_pin_alone", async () => {
    start({ ...NPM, applyFails: "the checksum did not match" });
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      /the checksum did not match/,
    );
  });

  it("test13_while_it_runs_the_dialog_says_it_may_be_closed", async () => {
    start({ ...NPM, delayMs: 600_000 });
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    expect(await within(dialog).findByText("Updating alpha-api")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/You can close this window: the update keeps running/),
    ).toBeInTheDocument();
  });
});

describe("the Updates row of a package", () => {
  it("test20_the_servers_banner_opens_the_plan_and_skip_stays_in_the_menu", async () => {
    start(NPM, "package-update");
    render(
      <>
        <PlusFooterBell />
        <AttentionBanner page="servers" />
      </>,
    );
    expect(
      await screen.findByText("alpha-api: @scope/alpha 1.5.0 is out, pinned to 1.4.2"),
    ).toBeInTheDocument();
    await userEvent.click(
      await screen.findByRole("button", {
        name: "Actions for alpha-api: @scope/alpha 1.5.0 is out, pinned to 1.4.2",
      }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "Skip 1.5.0" }),
    ).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    await userEvent.click(screen.getByRole("button", { name: "Update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByLabelText("Update alpha-api")).toBeChecked();
    expect(within(dialog).getByText("1.4.2 to 1.5.0")).toBeInTheDocument();
    expect(updateArgv().every((argv) => argv.includes("--dry-run"))).toBe(true);
  });

  it("test21_the_bell_opens_the_same_plan_from_its_popover", async () => {
    start(NPM, "package-update");
    render(<PlusFooterBell />);
    await userEvent.click(
      await screen.findByRole("button", { name: /^Needs attention/ }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Update" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByLabelText("Update alpha-api")).toBeChecked();
  });

  it("test22_a_release_row_offers_the_update_and_one_that_cannot_be_updated_says_what_to_do", async () => {
    start(RELEASE, "release-update");
    const { unmount } = render(
      <>
        <PlusFooterBell />
        <AttentionBanner page="servers" />
      </>,
    );
    expect(
      await screen.findByText(
        "alpha-api: release v1.4.0 of acme/alpha is out, installed 1.3.0",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update" })).toBeInTheDocument();
    unmount();
    registerAttentionFeed(null);
    start(RELEASE, "release-manual");
    render(
      <>
        <PlusFooterBell />
        <AttentionBanner page="servers" />
      </>,
    );
    expect(
      await screen.findByText(/replace the installed file by hand/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update" })).toBeNull();
  });
});

describe("review updates of several servers", () => {
  const SECOND: PackageSeed = {
    kind: "pypi",
    package: "tools-mcp",
    from: "0.9.1",
    to: "1.0.0",
  };

  function startMany(
    seeds: Record<string, PackageSeed>,
    restart?: Record<string, RestartScript>,
  ) {
    const list = fixtureServers();
    list[0].name = "alpha-api";
    list[1].name = "beta-mail";
    resetPlusFixture({
      servers: { servers: list, packages: seeds, restart },
      attention: { scenario: "package-updates" },
    });
    render(
      <>
        <PlusFooterBell />
        <AttentionBanner page="servers" />
      </>,
    );
  }

  async function review() {
    await userEvent.click(await screen.findByRole("button", { name: "Review updates" }));
    return screen.findByRole("dialog");
  }

  it("test30_one_plan_lists_every_servers_change_and_one_confirm_runs_them_all", async () => {
    startMany({ "fixture-0": NPM, "fixture-1": SECOND });
    const dialog = await review();
    expect(await within(dialog).findByLabelText("Update alpha-api")).toBeChecked();
    expect(within(dialog).getByLabelText("Update beta-mail")).toBeChecked();
    expect(within(dialog).getByText("PyPI")).toBeInTheDocument();
    expect(within(dialog).getByText("@scope/alpha@1.5.0")).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Update 2 servers" }),
    );
    expect(await within(dialog).findByText("2 updated")).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        /alpha-api and beta-mail still run the old version until restarted/,
      ),
    ).toBeInTheDocument();
    const apply = updateArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply.slice(0, 4)).toEqual(["server", "update", "alpha-api", "beta-mail"]);
    expect(apply).toContain("--no-restart");
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo all" }));
    expect(await within(dialog).findByText("2 put back")).toBeInTheDocument();
    expect(updateArgv()).toContainEqual([
      "server",
      "update",
      "alpha-api",
      "beta-mail",
      "--undo",
      "--no-restart",
    ]);
  });

  it("test31_only_the_ticked_servers_run_and_a_single_one_goes_through_its_own_plan", async () => {
    startMany({ "fixture-0": NPM, "fixture-1": SECOND });
    const dialog = await review();
    await userEvent.click(await within(dialog).findByLabelText("Update beta-mail"));
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Update 1 server" }),
    );
    expect(await within(dialog).findByText("alpha-api updated")).toBeInTheDocument();
    const apply = updateArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply.slice(0, 3)).toEqual(["server", "update", "alpha-api"]);
    expect(apply).not.toContain("beta-mail");
  });

  it("test32_a_release_without_a_checksum_stays_unticked_until_it_is_allowed_and_a_refused_server_cannot_be_ticked", async () => {
    startMany({
      "fixture-0": { ...RELEASE, release: { ...RELEASE.release!, checksum: null } },
      "fixture-1": {
        ...SECOND,
        refusal: {
          code: "cannot_rewrite",
          message: "its arguments come from a launch recipe",
        },
      },
    });
    const dialog = await review();
    const release = await within(dialog).findByLabelText("Update alpha-api");
    expect(release).not.toBeChecked();
    expect(release).toBeDisabled();
    expect(within(dialog).getByLabelText("Update beta-mail")).toBeDisabled();
    expect(
      within(dialog).getByText(/its arguments come from a launch recipe/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "Update 0 servers" }),
    ).toBeDisabled();
    await userEvent.click(
      within(dialog).getByRole("checkbox", { name: /Install without verification/ }),
    );
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Update 1 server" }),
    );
    await within(dialog).findByText("alpha-api updated");
    expect(updateArgv().find((argv) => argv.includes("--plan-hash"))).toContain(
      "--allow-unverified",
    );
  });

  it("test33_one_that_did_not_go_through_is_named_and_the_others_still_get_their_restart_prompt", async () => {
    startMany({
      "fixture-0": NPM,
      "fixture-1": { ...SECOND, applyFails: "the checksum did not match" },
    });
    const dialog = await review();
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update 2 servers" }),
    );
    expect(await within(dialog).findByText("1 updated, 1 failed")).toBeInTheDocument();
    expect(within(dialog).getByText(/the checksum did not match/)).toBeInTheDocument();
    expect(
      within(dialog).getByText(/alpha-api still run the old version until restarted/),
    ).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Restart 1" })).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo all" }));
    await within(dialog).findByText(/1 put back/);
    expect(updateArgv()).toContainEqual([
      "server",
      "update",
      "alpha-api",
      "--undo",
      "--no-restart",
    ]);
  });

  it("test34_each_server_keeps_its_own_restart_state_and_a_busy_one_can_restart_at_once", async () => {
    startMany(
      { "fixture-0": NPM, "fixture-1": SECOND },
      { "fixture-0": { kind: "busy" } },
    );
    const dialog = await review();
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update 2 servers" }),
    );
    await within(dialog).findByText("2 updated");
    await userEvent.click(within(dialog).getByRole("button", { name: "Restart 2" }));
    expect(await within(dialog).findByText("Restarted")).toBeInTheDocument();
    expect(ranArgv()).toContainEqual(["server", "restart", "beta-mail", "--if-idle"]);
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Restart now" }),
    );
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["server", "restart", "alpha-api"]),
    );
    await waitFor(() => expect(within(dialog).getAllByText("Restarted")).toHaveLength(2));
  });

  it("test35_the_summary_card_counts_updates_and_names_each_with_its_version", async () => {
    startMany({ "fixture-0": NPM, "fixture-1": SECOND });
    expect(await screen.findByText("2 updates are available")).toBeInTheDocument();
    expect(screen.getByText(/alpha-api 1.5.0, beta-mail 1.0.0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Review updates" })).toBeInTheDocument();
  });
});
