import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import { button, confirm, offline, openSystem } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Updates tab walked the way a person uses it, against a world with a git server, a
 * release, an npx and a uvx server and one whose source is unknown. Each test is named by the
 * parity action it proves (`src/plus/gui-parity.json`) and asserts that the next read changed. */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter(
      (line) =>
        !/^(commands|sync|update|cc|council|mcp|import|secret set council)( |$)/.test(
          line,
        ),
    );
  expect(stray, "the screen only runs its own command groups").toEqual([]);
});

const row = (id: string) => screen.getByRole("row", { name: new RegExp(`^${id}`) });
const state = () => bridge.world!.state.updates;

describe("Updates tab, end to end", () => {
  it("system.update-check: every source kind is listed, the update command is shown, one server is checked again", async () => {
    const user = await openSystem(bridge, "Updates");
    await screen.findByText("srv-git");
    for (const [id, kind] of [
      ["srv-git", "git"],
      ["srv-release", "GitHub release"],
      ["srv-npx", "npx"],
      ["srv-uvx", "uvx"],
      ["srv-new", "Unknown source"],
    ]) {
      expect(within(row(id)).getByText(kind)).toBeVisible();
    }
    expect(within(row("srv-git")).getByText("Update available")).toBeVisible();
    expect(within(row("srv-git")).getByText(/Update command:/)).toHaveTextContent(
      "./build.sh (only runs if you allow it)",
    );
    expect(within(row("srv-npx")).getByText("Automatic")).toBeVisible();
    expect(bridge.ran().filter((line) => line.startsWith("update"))).toEqual([
      "update --check",
    ]);
    await user.click(button("Check srv-npx"));
    await waitFor(() => expect(bridge.count("update srv-npx --check")).toBe(1));
    await user.click(button("Check all"));
    await waitFor(() => expect(bridge.count("update --check")).toBe(2));
  });

  it("system.update-check: a preview shows the update command and changes nothing, Escape cancels, the apply moves the server", async () => {
    const user = await openSystem(bridge, "Updates");
    await screen.findByText("srv-git");
    const opener = button("Update srv-git");
    await user.click(opener);
    const options = await screen.findByRole("dialog", { name: "Update srv-git" });
    expect(
      within(options).getByRole("list", { name: "Update commands" }),
    ).toHaveTextContent("./build.sh");
    await user.click(within(options).getByRole("button", { name: "Preview update" }));
    const box = await screen.findByRole("dialog", { name: "Update srv-git?" });
    expect(
      await within(box).findByText(
        /Update command: .\/build.sh \(not run without --allow-commands\)/,
      ),
    ).toBeVisible();
    expect(bridge.ran()).toContain("update srv-git --apply --dry-run");
    expect(bridge.count("update srv-git --apply")).toBe(0);
    expect(state()[0].status).toBe("update-available");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();

    await user.click(opener);
    const again = await screen.findByRole("dialog", { name: "Update srv-git" });
    await user.click(
      within(again).getByLabelText(/^Run the update command after the update/),
    );
    await user.click(within(again).getByRole("button", { name: "Preview update" }));
    await screen.findByRole("dialog", { name: "Update srv-git?" });
    await confirm(user, "Update", { done: /Done: git merge, moved to f6e5d4c3b2a1/ });
    expect(bridge.ran()).toContain("update srv-git --apply --allow-commands");
    await waitFor(() =>
      expect(within(row("srv-git")).getByText("Up to date")).toBeVisible(),
    );
    expect(screen.queryByRole("button", { name: "Update srv-git" })).toBeNull();
    expect(button("Update all…")).toBeEnabled();
  });

  it("system.update-check: Update all applies every pending server, and a release can be accepted unverified", async () => {
    const user = await openSystem(bridge, "Updates");
    await screen.findByText("srv-git");
    await user.click(button("Update all…"));
    const options = await screen.findByRole("dialog", {
      name: "Update every server with an update",
    });
    expect(
      within(options).getByRole("list", { name: "Servers to update" }),
    ).toHaveTextContent("srv-release");
    await user.click(
      within(options).getByLabelText(/^Accept updates that could not be verified/),
    );
    await user.click(within(options).getByRole("button", { name: "Preview update" }));
    await confirm(user, "Update", { done: /^Updated 2 servers/ });
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "update --apply --allow-unverified --dry-run",
        "update --apply --allow-unverified",
      ]),
    );
    await waitFor(() => expect(button("Update all…")).toBeDisabled());
    expect(within(row("srv-release")).getByText("Up to date")).toBeVisible();
  });

  it("system.cc-update: a plugin update previews first and the next list shows it current", async () => {
    const user = await openSystem(bridge, "Updates");
    const card = within(
      await screen.findByRole("group", { name: "Claude Code plugins" }),
    );
    const list = await card.findByRole("list", { name: "Plugin updates" });
    expect(within(list).getByText("update available")).toBeVisible();
    await user.click(card.getByRole("button", { name: "Update…" }));
    const box = await screen.findByRole("dialog", { name: "Update demo-plugin?" });
    expect(
      await within(box).findByText(/demo-plugin@fake-market: 1\.0\.0 to 1\.1\.0/),
    ).toBeVisible();
    expect(bridge.count("cc update demo-plugin")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Update" }));
    await screen.findByText(/Restart Claude Code/);
    expect(bridge.count("cc update demo-plugin")).toBe(1);
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Plugin updates" })).getByText(
          "up to date",
        ),
      ).toBeVisible(),
    );
  });

  it("system.update-check: detecting sources previews first and stores the source of the unknown server", async () => {
    const user = await openSystem(bridge, "Updates");
    await screen.findByText("srv-new");
    await user.click(button("Detect sources…"));
    const options = await screen.findByRole("dialog", { name: "Detect update sources" });
    await user.click(within(options).getByRole("button", { name: "Preview detection" }));
    const box = await screen.findByRole("dialog", { name: "Detect update sources?" });
    expect(await within(box).findByText(/stored source github-release/)).toBeVisible();
    expect(state().at(-1)!.detected).toBe(false);
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    await user.click(button("Detect sources…"));
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Detect update sources" }),
      ).getByRole("button", {
        name: "Preview detection",
      }),
    );
    await confirm(user, "Store sources");
    await waitFor(() =>
      expect(within(row("srv-new")).getByText("GitHub release")).toBeVisible(),
    );
    expect(bridge.ran()).toEqual(
      expect.arrayContaining(["update --init --dry-run", "update --init"]),
    );
  });

  it("system.update-check: the failure of a check is said, Retry recovers, and offline is noted", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    const online = offline(true);
    const view = userEvent.setup();
    render(<PlusViews view="system" onSelectView={() => {}} />);
    await view.click(
      within(await screen.findByRole("tablist", { name: "System sections" })).getByRole(
        "tab",
        {
          name: "Updates",
        },
      ),
    );
    const failed = (await screen.findByText("Couldn't check for updates")).closest(
      '[role="alert"]',
    ) as HTMLElement;
    expect(screen.getByText(/You are offline/)).toBeVisible();
    down = false;
    await view.click(within(failed).getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("srv-git")).toBeVisible();
    online.mockRestore();
  });
});
