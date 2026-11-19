import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { PackageSeed } from "../fixtures/packageWorld";
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
) {
  const list = fixtureServers();
  list[0].name = "alpha-api";
  resetPlusFixture({
    servers: { servers: list, packages: { "fixture-0": seed } },
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
    expect(
      await within(dialog).findByText(/Update @scope\/alpha from 1.4.2 to 1.5.0/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("args[1]")).toBeInTheDocument();
    expect(within(dialog).getByText("@scope/alpha@1.4.2")).toBeInTheDocument();
    expect(within(dialog).getByText("@scope/alpha@1.5.0")).toBeInTheDocument();
    expect(
      within(dialog).getByText(/Nothing is running that a restart would cut/),
    ).toBeInTheDocument();
    const plan = updateArgv()[0];
    expect(plan).toEqual(
      expect.arrayContaining(["server", "update", "alpha-api", "--dry-run", "--fetch"]),
    );
    expect(updateArgv().filter((argv) => !argv.includes("--dry-run"))).toHaveLength(0);
  });

  it("test02_confirming_applies_the_reviewed_plan_and_offers_undo_which_puts_it_back", async () => {
    start(NPM);
    open();
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Update alpha-api" }),
    );
    expect(
      await within(dialog).findByText(/Updated alpha-api: @scope\/alpha 1.4.2 to 1.5.0/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText(/Restarted alpha-api/)).toBeInTheDocument();
    const apply = updateArgv().find((argv) => argv.includes("--plan-hash"))!;
    expect(apply).toEqual([
      "server",
      "update",
      "alpha-api",
      "--to",
      "1.5.0",
      "--plan-hash",
      "sha256:1.4.2>1.5.0",
    ]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo" }));
    expect(
      await within(dialog).findByText(/Put back alpha-api: @scope\/alpha 1.5.0 to 1.4.2/),
    ).toBeInTheDocument();
    expect(updateArgv()).toContainEqual(["server", "update", "alpha-api", "--undo"]);
    expect(within(dialog).queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("test03_a_release_without_a_checksum_waits_for_the_person_to_allow_it", async () => {
    start({ ...RELEASE, release: { ...RELEASE.release!, checksum: null } });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(
      (await within(dialog).findAllByText("alpha_1.4.0_darwin_arm64.tar.gz")).length,
    ).toBeGreaterThan(0);
    const apply = within(dialog).getByRole("button", { name: "Update alpha-api" });
    expect(apply).toBeDisabled();
    await userEvent.click(within(dialog).getByRole("checkbox"));
    expect(apply).toBeEnabled();
    await userEvent.click(apply);
    await within(dialog).findByText(/Updated alpha-api/);
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
    expect(within(dialog).getByText("mcpmSource.current_version")).toBeInTheDocument();
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
    expect(within(dialog).getByText(/12 MB/)).toBeInTheDocument();
  });

  it("test05_a_plan_that_cannot_be_applied_says_why_and_offers_no_update", async () => {
    start({
      ...NPM,
      refusal: {
        code: "cannot_rewrite",
        message:
          "Toolport cannot change this pin itself: its arguments come from a catalog launch recipe",
      },
    });
    open();
    expect(await screen.findByRole("alert")).toHaveTextContent(/launch recipe/);
    expect(screen.getByRole("button", { name: "Update alpha-api" })).toBeDisabled();
  });

  it("test06_a_server_already_on_the_newest_version_says_nothing_is_to_do", async () => {
    start({ ...NPM, to: "1.4.2" });
    open();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      /Nothing to update: @scope\/alpha is already on 1.4.2/,
    );
  });

  it("test07_an_unreachable_registry_shows_its_reason_and_changes_nothing", async () => {
    start({ ...NPM, offline: "registry.npmjs.org: could not connect (no route)" });
    open();
    expect(await screen.findByRole("alert")).toHaveTextContent(/no route/);
    expect(updateArgv().filter((argv) => !argv.includes("--dry-run"))).toHaveLength(0);
  });

  it("test08_a_plan_that_changed_since_the_review_is_shown_again_instead_of_applied", async () => {
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
  });

  it("test09_a_running_call_keeps_the_restart_back_until_the_person_asks", async () => {
    start({ ...NPM, inFlight: 1 });
    open();
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/1 call running now/)).toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Update alpha-api" }),
    );
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Restart now" }),
    );
    await waitFor(() =>
      expect(ranArgv()).toContainEqual(["server", "restart", "alpha-api"]),
    );
  });
});

describe("the Updates row of a package", () => {
  it("test10_the_servers_banner_opens_the_plan_and_skip_stays_in_the_menu", async () => {
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
    expect(
      await within(dialog).findByText(/Update @scope\/alpha from 1.4.2 to 1.5.0/),
    ).toBeInTheDocument();
    expect(updateArgv().every((argv) => argv.includes("--dry-run"))).toBe(true);
  });

  it("test11_the_bell_opens_the_same_plan_from_its_popover", async () => {
    start(NPM, "package-update");
    render(<PlusFooterBell />);
    await userEvent.click(
      await screen.findByRole("button", { name: /^Needs attention/ }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Update" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Update @scope\/alpha from 1.4.2 to 1.5.0/),
    ).toBeInTheDocument();
  });

  it("test12_a_release_row_offers_the_update_and_one_that_cannot_be_updated_says_what_to_do", async () => {
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
