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
import { invokeLog, ranArgv } from "./testkit";

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

const OTHER = { machine: "laptop", branch: "feature", head: "77aa11bb22cc" };

function start(seed: RepoSeed) {
  const list = fixtureServers();
  list[0].name = "alpha-api";
  resetPlusFixture({
    attention: { scenario: "peer-branch" },
    servers: { servers: list, repos: { "fixture-0": { other: OTHER, ...seed } } },
  });
  render(
    <>
      <PlusFooterBell />
      <AttentionBanner page="servers" />
    </>,
  );
}

const switchArgv = () => ranArgv().filter((argv) => argv[1] === "repo");

beforeEach(() => {
  invokeLog.length = 0;
  registerAttentionFeed(null);
});

describe("servers.repo-switch", () => {
  it("test00_the_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["servers.repo-switch"]).toMatchObject({
      status: "built",
      test: "src/plus/servers/RepoSwitchDialog.test.tsx",
    });
  });

  it("test01_the_banner_button_opens_a_plan_that_fetches_and_changes_nothing", async () => {
    start({});
    await userEvent.click(await screen.findByRole("button", { name: "Switch branch" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/laptop is on feature at 77aa11bb/),
    ).toBeInTheDocument();
    expect(switchArgv().every((argv) => argv.includes("--dry-run"))).toBe(true);
    expect(switchArgv()[0]).toContain("--fetch");
  });

  it("test02_confirming_switches_with_the_plan_hash_and_undo_goes_back", async () => {
    start({});
    await userEvent.click(await screen.findByRole("button", { name: "Switch branch" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Switch to feature" }),
    );
    expect(
      await within(dialog).findByText("alpha-api is on feature"),
    ).toBeInTheDocument();
    const applied = switchArgv().find((argv) => !argv.includes("--dry-run"));
    expect(applied).toContain("--plan-hash");
    await userEvent.click(within(dialog).getByRole("button", { name: "Undo" }));
    expect(
      await within(dialog).findByText("alpha-api is back on main"),
    ).toBeInTheDocument();
    expect(switchArgv().some((argv) => argv.includes("--undo"))).toBe(true);
  });

  it("test03_local_changes_refuse_the_switch_and_offer_no_confirm_button", async () => {
    start({ dirty: 2 });
    await userEvent.click(await screen.findByRole("button", { name: "Switch branch" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(
        /Toolport will not switch this checkout: local changes/,
      ),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: /^Switch to/ })).toBeNull();
    await waitFor(() => expect(switchArgv().length).toBe(1));
  });
});
