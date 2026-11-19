import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import type { PackageSeed } from "../fixtures/packageWorld";
import { fixtureServers } from "../fixtures/serverWorld";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "./testkit";
import SourceRow from "./SourceRow";

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

function start(seed?: PackageSeed) {
  const list = fixtureServers();
  list[0].name = "alpha-api";
  resetPlusFixture({
    servers: { servers: list, packages: seed ? { "fixture-0": seed } : {} },
    attention: { scenario: "clear" },
  });
  render(<SourceRow serverId="fixture-0" />);
}

const sourceArgv = () => ranArgv().filter((argv) => argv[1] === "source");

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("servers.source-row", () => {
  it("test00_the_action_is_built_and_both_surfaces_name_it", () => {
    expect(guiParity.actions["servers.source-row"]).toMatchObject({
      status: "built",
      test: "src/plus/servers/SourceRow.test.tsx",
    });
    expect(guiParity.commands["server source"].action).toBe("servers.source-row");
    expect(guiParity.tools.servers_source.action).toBe("servers.source-row");
  });

  it("test01_a_pin_with_a_newer_version_says_so_and_updates_through_the_plan", async () => {
    start(NPM);
    expect(
      await screen.findByText(/npm @scope\/alpha 1.4.2 · 1.5.0 is out/),
    ).toBeInTheDocument();
    expect(screen.getByText("Update available")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Update" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/Update @scope\/alpha from 1.4.2 to 1.5.0/),
    ).toBeInTheDocument();
    expect(sourceArgv()[0]).toEqual(["server", "source", "fixture-0"]);
  });

  it("test02_a_current_pin_shows_when_it_was_checked_and_offers_only_check_now", async () => {
    start({ ...NPM, to: "1.4.2" });
    expect(await screen.findByText(/up to date, checked/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Check now" }));
    await waitFor(() => expect(ranArgv()).toContainEqual(["attention", "check"]));
    await waitFor(() => expect(sourceArgv().length).toBeGreaterThan(1));
  });

  it("test03_a_failed_check_names_the_reason_and_a_pin_never_checked_says_so", async () => {
    start({ ...NPM, sourceState: "error", sourceError: "registry.npmjs.org: no route" });
    expect(
      await screen.findByText(/the last check failed: registry.npmjs.org: no route/),
    ).toBeInTheDocument();
  });

  it("test04_a_pin_toolport_cannot_rewrite_gets_no_update_button_and_says_why", async () => {
    start({
      ...NPM,
      refusal: {
        code: "cannot_rewrite",
        message: "its arguments come from a launch recipe",
      },
    });
    expect(
      await screen.findByText(
        /cannot update it itself: its arguments come from a launch recipe/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update" })).toBeNull();
  });

  it("test05_an_image_pinned_by_digest_shows_the_tag_it_follows_and_a_short_digest", async () => {
    start({
      kind: "docker",
      package: "acme/files",
      tracks: "latest",
      from: `sha256:${"a".repeat(64)}`,
      to: `sha256:${"b".repeat(64)}`,
    });
    expect(
      await screen.findByText(
        /Docker image acme\/files:latest sha256:aaaaaaaaaaaa\.\.\. · sha256:bbbbbbbbbbbb\.\.\. is out/,
      ),
    ).toBeInTheDocument();
  });

  it("test06_a_runner_that_fetches_the_newest_version_has_nothing_to_apply_and_no_source_says_so", async () => {
    start({ ...NPM, sourceState: "automatic" });
    expect(
      await screen.findByText(/fetches the newest version on each start/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
  });

  it("test07_a_server_with_no_known_source_gets_one_quiet_line", async () => {
    start();
    expect(
      await screen.findByText("No update source is known for this server"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
