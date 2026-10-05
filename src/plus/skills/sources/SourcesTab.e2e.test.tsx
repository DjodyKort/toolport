import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { openLibrary } from "../e2e";
import { createBridge, wire, type Bridge } from "../testkit";
import { ADD_ROOT, REMOVE_ROOT } from "./world";

/** The Sources tab walked through the Library screen the way a person uses it, against the
 * stateful world: an applied `sources root add|rm` changes the next read, a preview never does.
 * Each describe is named by the parity action it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(
    bridge
      .ran()
      .some((line) => /--home|--data-dir|secret|--reveal|stdin|token/.test(line)),
  ).toBe(false);
});

async function sources() {
  const user = await openLibrary();
  await user.click(screen.getByRole("tab", { name: "Sources" }));
  await screen.findByRole("list", { name: "Sources" });
  return user;
}
const sourceList = () => screen.getByRole("list", { name: "Sources" });
const folders = () => screen.getByRole("list", { name: "Scanned folders" });
const rowButton = (name: string) =>
  within(sourceList())
    .getAllByRole("button")
    .find((b) => within(b).queryByText(name, { selector: "b" }))!;
const detail = (name: string) => screen.getByRole("region", { name: `Source ${name}` });

describe("sources.ls", () => {
  it("opens on the Sources tab with every place Toolport looks, marked read-only or editable", async () => {
    await sources();
    expect(screen.getByRole("tab", { name: "Sources" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(within(sourceList()).getAllByRole("listitem")).toHaveLength(11);
    expect(within(rowButton("corp-tools")).getByText("read-only")).toBeVisible();
    expect(within(rowButton("ai-skills")).queryByText("read-only")).toBeNull();
    expect(within(rowButton("odh")).getByText("behind its remote (114)")).toBeVisible();
    expect(bridge.count("sources ls")).toBeGreaterThanOrEqual(1);
  });

  it("shows the library row with its duplicate clone and Pull and Push off", async () => {
    const user = await sources();
    await user.click(rowButton("ai-skills"));
    const box = detail("ai-skills");
    expect(within(box).getByText(/a second clone of the same remote/)).toBeVisible();
    expect(within(box).getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(within(box).getByRole("button", { name: "Push…" })).toBeDisabled();
    expect(within(box).getByText("3 of 4 skills")).toBeVisible();
  });

  it("lists the items of a source on demand and rescans with --refresh", async () => {
    const user = await sources();
    await user.click(rowButton("corp-tools"));
    await user.click(
      within(detail("corp-tools")).getByRole("button", { name: "Show items" }),
    );
    const items = await within(detail("corp-tools")).findByRole("list", {
      name: "Items",
    });
    expect(within(items).getByText("odoo-upgrade")).toBeVisible();
    expect(bridge.count("sources ls --source org --items")).toBe(1);
    await user.click(screen.getByRole("button", { name: "Rescan sources" }));
    await screen.findByRole("list", { name: "Sources" });
    expect(bridge.count("sources ls --refresh")).toBe(1);
  });
});

describe("sources.root.ls", () => {
  it("lists the folders that are scanned, which are defaults and which you added", async () => {
    await sources();
    const items = within(
      await screen.findByRole("list", { name: "Scanned folders" }),
    ).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(within(items[0]).getByText("default")).toBeVisible();
    expect(within(items[2]).getByText("added by you")).toBeVisible();
    expect(bridge.count("sources root ls")).toBe(1);
  });
});

describe("sources.root.add", () => {
  it("previews the folder, changes nothing until Add folder, then lists it", async () => {
    const user = await sources();
    const before = await screen.findByRole("list", { name: "Scanned folders" });
    await user.click(screen.getByRole("button", { name: "Add folder to scan…" }));
    const form = await screen.findByRole("dialog", { name: /^Add a folder to scan$/ });
    await user.type(within(form).getByLabelText("Folder"), ADD_ROOT);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /^Add a folder to scan\?$/ });
    expect(within(box).getByText(`add source root ${ADD_ROOT}`)).toBeVisible();
    expect(bridge.count(`sources root add ${ADD_ROOT} --dry-run`)).toBe(1);
    expect(before.querySelectorAll("li")).toHaveLength(3);
    await user.click(within(box).getByRole("button", { name: "Add folder" }));
    await screen.findByText("Done");
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() =>
      expect(within(folders()).getAllByRole("listitem")).toHaveLength(4),
    );
    expect(within(folders()).getByText("client-repo")).toBeVisible();
    expect(bridge.count(`sources root add ${ADD_ROOT}`)).toBe(1);
  });
});

describe("sources.root.rm", () => {
  it("previews, asks for the folder's name, then stops scanning it", async () => {
    const user = await sources();
    const before = await screen.findByRole("list", { name: "Scanned folders" });
    await user.click(screen.getByRole("button", { name: "Stop scanning dups" }));
    const box = await screen.findByRole("dialog", { name: /^Stop scanning dups\?$/ });
    expect(within(box).getByText(`remove source root ${REMOVE_ROOT}`)).toBeVisible();
    expect(before.querySelectorAll("li")).toHaveLength(3);
    const confirm = within(box).getByRole("button", { name: "Stop scanning" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "dups");
    await user.click(confirm);
    await screen.findByText("Done");
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() =>
      expect(within(folders()).getAllByRole("listitem")).toHaveLength(2),
    );
    expect(screen.queryByRole("button", { name: "Stop scanning dups" })).toBeNull();
    expect(bridge.count(`sources root rm ${REMOVE_ROOT}`)).toBe(1);
  });
});
