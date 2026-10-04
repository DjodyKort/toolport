import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { confirm, group, openSystem } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Import tab walked the way a person uses it: the mcpm importer with its preview, the
 * reference rewrite with its orphan report, and the way back. Each test is named by the
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
        !/^(commands|sync|update|council|mcp|import|secret set council)( |$)/.test(line),
    );
  expect(stray, "the screen only runs its own command groups").toEqual([]);
});

const ROOT = "/old/mcpm";
const TOOLS = "/old/tools.json";

describe("Import tab, end to end", () => {
  it("system.import-mcpm: the preview is the dry run, shows what is rejected, writes nothing; the apply makes the next preview unchanged", async () => {
    const user = await openSystem(bridge, "Import");
    const card = group("Import from mcpm");
    expect(card.getByRole("button", { name: "Preview import…" })).toBeDisabled();
    await user.type(card.getByLabelText("mcpm config folder"), ROOT);
    const opener = card.getByRole("button", { name: "Preview import…" });
    await user.click(opener);
    const box = await screen.findByRole("dialog", { name: "Import from mcpm?" });
    expect(
      await within(box).findByText(/Import 2 servers, 1 profile and 0 secrets/),
    ).toBeVisible();
    const dry = bridge.world!.reply(["import", "mcpm", ROOT, "--dry-run"]) as {
      servers: Array<{ id: string }>;
      profiles: Array<{ id: string }>;
    };
    for (const entry of dry.servers) {
      expect(within(box).getByText(`Server ${entry.id}: created`)).toBeVisible();
    }
    expect(within(box).getByText(`Profile ${dry.profiles[0].id}: created`)).toBeVisible();
    expect(
      within(box).getByText(/Not imported, gamma-mock: its launcher is not on the path/),
    ).toBeVisible();
    expect(bridge.ran()).toContain(`import mcpm ${ROOT} --dry-run`);
    expect(bridge.count(`import mcpm ${ROOT}`)).toBe(0);
    expect(bridge.world!.state.mcpm.imported).toBe(false);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();

    await user.click(opener);
    await confirm(user, "Import", { done: /^Imported 2 servers, 1 profile/ });
    expect(bridge.count(`import mcpm ${ROOT}`)).toBe(1);
    expect(bridge.world!.state.mcpm.imported).toBe(true);
    await user.click(opener);
    const again = await screen.findByRole("dialog", { name: "Import from mcpm?" });
    expect(await within(again).findByText("Server alpha-mock: unchanged")).toBeVisible();
    await user.click(within(again).getByRole("button", { name: "Cancel" }));
  });

  it("system.import-mcpm: the name map lists the old and the new tool names", async () => {
    const user = await openSystem(bridge, "Import");
    await user.type(group("Import from mcpm").getByLabelText("mcpm config folder"), ROOT);
    const rewrite = group("Rewrite tool references");
    await user.type(rewrite.getByLabelText("Tools file"), TOOLS);
    await user.click(rewrite.getByRole("button", { name: "Show name map" }));
    const map = await screen.findByRole("list", { name: "Name map" });
    expect(within(map).getByText("mcp__mcpm_alpha-mock__add")).toBeVisible();
    expect(within(map).getByText("mcp__toolport__alpha_mock__add")).toBeVisible();
    expect(bridge.ran()).toContain(
      `import mcpm ${ROOT} --dry-run --tools ${TOOLS} --name-map`,
    );
  });

  it("system.import-rename-refs: the preview names a wildcard orphan and changes nothing, the apply rewrites and the next preview finds nothing", async () => {
    const user = await openSystem(bridge, "Import");
    await user.type(group("Import from mcpm").getByLabelText("mcpm config folder"), ROOT);
    const rewrite = group("Rewrite tool references");
    await user.type(rewrite.getByLabelText("Tools file"), TOOLS);
    await user.type(
      rewrite.getByLabelText("Files or folders to rewrite"),
      "/notes{Enter}/project",
    );
    const opener = rewrite.getByRole("button", { name: "Preview rewrite…" });
    await user.click(opener);
    const box = await screen.findByRole("dialog", { name: "Rewrite tool references?" });
    expect(
      await within(box).findByText("Rewrite 4 references in 2 files (3 scanned)"),
    ).toBeVisible();
    expect(bridge.ran()).toContain(
      `import rename-refs ${ROOT} --tools ${TOOLS} --paths /notes /project --dry-run`,
    );
    expect(bridge.world!.state.mcpm.rewritten).toBe(false);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();
    const report = await screen.findByRole("group", {
      name: "Orphan report (from the preview)",
    });
    expect(within(report).getByText("mcp__mcpm_acme-erp__*")).toBeVisible();
    expect(within(report).getByText("in a ask rule")).toBeVisible();

    await user.click(opener);
    await confirm(user, "Rewrite", { done: /^Rewrote 4 references in 2 files/ });
    expect(bridge.world!.state.mcpm.rewritten).toBe(true);
    expect(
      await screen.findByRole("group", { name: "Orphan report (from the rewrite)" }),
    ).toBeVisible();
    await user.click(opener);
    expect(
      await screen.findByText("Rewrite 0 references in 0 files (3 scanned)"),
    ).toBeVisible();
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
  });

  it("system.import-mcpm: the undo is a terminal command with a disabled Open in Terminal, and the other importers are marked not implemented", async () => {
    const user = await openSystem(bridge, "Import");
    const undo = group("Undo");
    await user.type(
      undo.getByLabelText("Backup folder (optional)"),
      "/backups/cutover-1",
    );
    expect(undo.getByLabelText("Command line")).toHaveTextContent(
      "scripts/cutover/rollback.sh --home ~ --backup /backups/cutover-1",
    );
    expect(undo.getByRole("button", { name: /Open in Terminal/ })).toBeDisabled();
    expect(undo.getByRole("button", { name: "Copy command" })).toBeVisible();
    expect(group("Other tools").getByText("Not implemented")).toBeVisible();
  });
});
