import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { goldenData, updateWorld } from "./fixtures";
import { open } from "./harness";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";
import { UpdatesTab } from "./UpdatesTab";

let bridge: Bridge;
const ccList = goldenData("cc-list") as { plugins: unknown[] };
const onOpenCommands = vi.fn();

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
  onOpenCommands.mockReset();
});

const rowOf = (id: string) =>
  screen
    .getAllByRole("row")
    .find((row) => within(row).queryByText(id, { selector: "th" }))!;

describe("Updates tab: reading", () => {
  it("lists every server with its source, state and version, from the golden check", async () => {
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(await screen.findByText("srv-alpha")).toBeInTheDocument();
    expect(within(rowOf("srv-alpha")).getByText("Unknown source")).toBeInTheDocument();
    expect(within(rowOf("srv-beta")).getByText("Remote server")).toBeInTheDocument();
    expect(within(rowOf("srv-beta")).getByText("Skipped")).toBeInTheDocument();
    expect(screen.getByText("2 skipped")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update all…" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: /^Update srv/ })).toBeNull();
  });

  it("shows the update command of a server next to its state, and who can update", async () => {
    bridge.set("update --check", updateWorld);
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    const git = rowOf("srv-git");
    expect(within(git).getByText("Update available")).toBeInTheDocument();
    expect(within(git).getByText("./build.sh")).toBeInTheDocument();
    expect(within(git).getByText(/only runs if you allow it/)).toBeInTheDocument();
    expect(within(git).getByText("2 commit(s) behind origin/main")).toBeInTheDocument();
    expect(within(rowOf("srv-release")).getByText("GitHub release")).toBeInTheDocument();
    expect(within(rowOf("srv-npx")).getByText("npx")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Update srv/ })).toHaveLength(1);
  });

  it("says there is nothing to update when no server is listed", async () => {
    bridge.set("update --check", { mode: "check", counts: {}, servers: [] });
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(await screen.findByText("No servers to update")).toBeInTheDocument();
  });

  it("shows the failure of the check with Retry and still shows the plugins section", async () => {
    let fail = true;
    bridge.set("update --check", () =>
      fail ? failure("update", "could not reach the network") : updateWorld,
    );
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(await screen.findByText("could not reach the network")).toBeInTheDocument();
    expect(
      screen.getByRole("group", { name: "Claude Code plugins" }),
    ).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("srv-git")).toBeInTheDocument();
  });

  it("says when the machine is offline", async () => {
    const online = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(await screen.findByText(/You are offline/)).toBeInTheDocument();
    online.mockRestore();
  });

  it("lists the Claude Code plugins with the server updates", async () => {
    bridge.set("update --check", updateWorld);
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    const card = await screen.findByRole("group", { name: "Claude Code plugins" });
    const list = await within(card).findByRole("list", { name: "Plugin updates" });
    expect(within(list).getByText("demo-plugin")).toBeInTheDocument();
    expect(within(list).getByText(/1\.0\.0 · fake-market/)).toBeInTheDocument();
    expect(within(list).getByText("update state unknown")).toBeInTheDocument();
    expect(await screen.findByText("srv-git")).toBeInTheDocument();
    expect(bridge.count("cc list")).toBe(1);
  });

  it("updates one plugin through cc update after a preview", async () => {
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    const card = await screen.findByRole("group", { name: "Claude Code plugins" });
    await user.click(await within(card).findByRole("button", { name: "Update…" }));
    const box = await screen.findByRole("dialog", { name: /^Update demo-plugin\?$/ });
    expect(within(box).getByText(/demo-plugin@fake-market: 1\.0\.0/)).toBeInTheDocument();
    expect(bridge.count("cc update demo-plugin --dry-run")).toBe(1);
    expect(bridge.count("cc update demo-plugin")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Update" }));
    expect(await screen.findByText(/Restart Claude Code/)).toBeInTheDocument();
    expect(bridge.count("cc update demo-plugin")).toBe(1);
  });

  it("previews updating every plugin and applies nothing until confirmed", async () => {
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    const card = await screen.findByRole("group", { name: "Claude Code plugins" });
    await within(card).findByRole("list", { name: "Plugin updates" });
    await user.click(within(card).getByRole("button", { name: "Update all plugins…" }));
    const box = await screen.findByRole("dialog", { name: /^Update all plugins\?$/ });
    expect(bridge.count("cc update --dry-run")).toBe(1);
    expect(bridge.count("cc update")).toBe(0);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    expect(bridge.count("cc update")).toBe(0);
  });

  it("shows the failure of the plugin list with Retry", async () => {
    let fail = true;
    bridge.set("cc list", () =>
      fail ? failure("cc", "claude is not installed") : ccList,
    );
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(await screen.findByText("claude is not installed")).toBeInTheDocument();
    fail = false;
    const card = screen.getByRole("group", { name: "Claude Code plugins" });
    await user.click(within(card).getByRole("button", { name: "Retry" }));
    expect(await within(card).findByText("demo-plugin")).toBeInTheDocument();
  });

  it("says so when no plugin is installed", async () => {
    bridge.set("cc list", { ...ccList, plugins: [] });
    await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    expect(
      await screen.findByText("No Claude Code plugin is installed."),
    ).toBeInTheDocument();
  });

  it("checks one server again and shows its new row", async () => {
    bridge.set("update --check", updateWorld);
    bridge.set("update srv-npx --check", {
      mode: "check",
      counts: { "up-to-date": 1 },
      servers: [
        { ...updateWorld.servers[2], status: "up-to-date", message: "pinned, current" },
      ],
    });
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-npx");
    await user.click(screen.getByRole("button", { name: "Check srv-npx" }));
    expect(await screen.findByText("pinned, current")).toBeInTheDocument();
    expect(within(rowOf("srv-npx")).getByText("Up to date")).toBeInTheDocument();
  });

  it("shows why a single check failed without losing the table", async () => {
    bridge.set("update --check", updateWorld);
    bridge.set("update srv-git --check", failure("update", "git fetch failed"));
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Check srv-git" }));
    expect(await screen.findByText("srv-git: git fetch failed")).toBeInTheDocument();
    expect(rowOf("srv-git")).toBeInTheDocument();
  });
});

describe("Updates tab: apply with a preview", () => {
  it("shows the update command before it runs, previews with --dry-run and applies on confirm", async () => {
    bridge.set("update --check", updateWorld);
    const report = (mode: string, status: string, message: string) => ({
      mode,
      counts: { [status]: 1 },
      servers: [{ ...updateWorld.servers[0], status, message }],
    });
    bridge.set(
      "update srv-git --apply --dry-run",
      report("dry-run", "update-available", "2 commit(s) behind origin/main"),
    );
    bridge.set(
      "update srv-git --apply",
      report("apply", "updated", "updated (2 new commit(s))"),
    );
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Update srv-git" }));
    const options = await screen.findByRole("dialog");
    const commands = within(options).getByRole("list", { name: "Update commands" });
    expect(within(commands).getByText("./build.sh")).toBeInTheDocument();
    expect(
      within(options).getByRole("checkbox", { name: /Run the update command/ }),
    ).not.toBeChecked();
    await user.click(within(options).getByRole("button", { name: "Preview update" }));
    const box = await screen.findByRole("dialog", { name: "Update srv-git?" });
    expect(await within(box).findByText("Update 1 server")).toBeInTheDocument();
    expect(within(box).getByText(/Update command: .*build\.sh/)).toBeInTheDocument();
    expect(within(box).queryByRole("textbox")).toBeNull();
    await user.click(within(box).getByRole("button", { name: "Update" }));
    expect(await screen.findByText("Updated 1 server")).toBeInTheDocument();
    expect(bridge.ran().filter((line) => line.startsWith("update srv-git"))).toEqual([
      "update srv-git --apply --dry-run",
      "update srv-git --apply",
    ]);
    expect(bridge.count("update --check")).toBe(2);
  });

  it("lets the update command run only when the user ticks it, in the preview and the apply", async () => {
    bridge.set("update --check", updateWorld);
    const argv = "update srv-git --apply --allow-commands";
    bridge.set(`${argv} --dry-run`, { ...updateWorld, mode: "dry-run" });
    bridge.set(argv, { ...updateWorld, mode: "apply" });
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Update srv-git" }));
    const options = await screen.findByRole("dialog");
    await user.click(
      within(options).getByRole("checkbox", { name: /Run the update command/ }),
    );
    await user.click(within(options).getByRole("button", { name: "Preview update" }));
    const box = await screen.findByRole("dialog", { name: "Update srv-git?" });
    await within(box).findByText("Update 1 server");
    expect(within(box).getByLabelText("Command line")).toHaveTextContent(
      "--allow-commands",
    );
    await user.click(within(box).getByRole("button", { name: "Update" }));
    await screen.findByText("Updated 1 server");
    expect(bridge.ran()).toContain(argv);
  });

  it("updates every server with an update at once", async () => {
    bridge.set("update --check", updateWorld);
    bridge.set("update --apply --dry-run", { ...updateWorld, mode: "dry-run" });
    bridge.set("update --apply", { ...updateWorld, mode: "apply" });
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Update all…" }));
    const options = await screen.findByRole("dialog");
    expect(
      within(
        within(options).getByRole("list", { name: "Servers to update" }),
      ).getAllByRole("listitem"),
    ).toHaveLength(1);
    await user.click(within(options).getByRole("button", { name: "Preview update" }));
    const box = await screen.findByRole("dialog", {
      name: "Update every server with an update?",
    });
    await within(box).findByText("Update 1 server");
    await user.click(within(box).getByRole("button", { name: "Update" }));
    expect(await screen.findByText("Updated 1 server")).toBeInTheDocument();
    expect(bridge.ran().filter((line) => line.startsWith("update --apply"))).toEqual([
      "update --apply --dry-run",
      "update --apply",
    ]);
  });

  it("shows a failed preview and applies nothing", async () => {
    bridge.set("update --check", updateWorld);
    bridge.set("update srv-git --apply --dry-run", failure("update", "the remote moved"));
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Update srv-git" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Preview update",
      }),
    );
    expect(await screen.findByText("the remote moved")).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("update srv-git --apply");
  });

  it("detects the update sources with a preview and --force when asked", async () => {
    const argv = "update --init --force";
    bridge.set(`${argv} --dry-run`, { ...updateWorld, mode: "init" });
    bridge.set(argv, { ...updateWorld, mode: "init" });
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-alpha");
    await user.click(screen.getByRole("button", { name: "Detect sources…" }));
    const options = await screen.findByRole("dialog");
    await user.click(within(options).getByRole("checkbox", { name: /Detect again/ }));
    await user.click(within(options).getByRole("button", { name: "Preview detection" }));
    const box = await screen.findByRole("dialog", { name: "Detect update sources?" });
    await within(box).findByText("Update 1 server");
    await user.click(within(box).getByRole("button", { name: "Store sources" }));
    await screen.findByText("Updated 1 server");
    expect(bridge.ran()).toContain(argv);
  });

  it("refuses a repository that is not owner/repo before it previews", async () => {
    bridge.set("update --check", updateWorld);
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-git");
    await user.click(screen.getByRole("button", { name: "Update srv-git" }));
    const options = await screen.findByRole("dialog");
    await user.type(within(options).getByLabelText(/GitHub repository/), "not a repo");
    expect(within(options).getByText("Use the form owner/repo")).toBeInTheDocument();
    expect(
      within(options).getByRole("button", { name: "Preview update" }),
    ).toBeDisabled();
  });
});

describe("Updates tab: golden shapes", () => {
  it("shows the dry-run golden of update --apply as a plan of skipped servers", async () => {
    bridge.set("update --apply --dry-run", golden("update.preview"));
    const { user } = await open(<UpdatesTab onOpenCommands={onOpenCommands} />, bridge);
    await screen.findByText("srv-alpha");
    expect(screen.getByText("2 skipped")).toBeInTheDocument();
    expect(await screen.findAllByText(/run update --init/)).not.toHaveLength(0);
    expect(user).toBeDefined();
  });
});
