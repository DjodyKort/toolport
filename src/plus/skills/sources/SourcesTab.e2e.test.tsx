import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { openLibrary } from "../e2e";
import { createBridge, wire, type Bridge } from "../testkit";
import { ADD_ROOT, REMOVE_ROOT, type LibraryWorld } from "./world";

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
      .some(
        (line) =>
          !line.startsWith("mcp call ") &&
          /--home|--data-dir|secret|--reveal|stdin|token/.test(line),
      ),
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

  it("shows the library row with its duplicate clone and its remote", async () => {
    const user = await sources();
    await user.click(rowButton("ai-skills"));
    const box = detail("ai-skills");
    expect(within(box).getByText(/a second clone of the same remote/)).toBeVisible();
    expect(await within(box).findByText("Remote")).toBeVisible();
    expect(within(box).getByRole("list", { name: "Other clones" })).toBeVisible();
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

const library = () => within(screen.getByRole("group", { name: "Library remote" }));

async function libraryRow(world: LibraryWorld = {}) {
  bridge = createBridge({ world: { library: world } });
  wire({ invoke, listen }, bridge);
  const user = await sources();
  await user.click(rowButton("ai-skills"));
  await library().findByText("Changes here");
  return user;
}

async function finish(user: Awaited<ReturnType<typeof sources>>) {
  await screen.findByText("Done");
  await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

describe("sources.library.status", () => {
  it("reads the clone against its remote and reaches the remote only on Check the remote", async () => {
    const user = await libraryRow();
    expect(library().getByText("2 commits behind, 0 commits ahead")).toBeVisible();
    expect(library().getByText("Nothing changed here")).toBeVisible();
    expect(library().getByRole("list", { name: "Other clones" })).toBeVisible();
    expect(bridge.count("library status")).toBe(1);
    expect(bridge.count("library status --fetch")).toBe(0);
    await user.click(library().getByRole("button", { name: "Check the remote" }));
    expect(await library().findByText(/the remote accepted them/)).toBeVisible();
    expect(bridge.count("library status --fetch")).toBe(1);
  });

  it("counts the files changed here and the commits not pushed", async () => {
    await libraryRow({ uncommitted: 2, ahead: 1 });
    expect(library().getByText("2 files changed, 1 commit not pushed")).toBeVisible();
  });

  it("shows a clone without a remote and turns Pull and Push off", async () => {
    await libraryRow({ remote: false });
    expect(library().getByText("No remote")).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(library().getByRole("button", { name: "Push…" })).toBeDisabled();
  });
});

describe("sources.library.pull", () => {
  it("previews the commits, pulls on confirm, and the next read is level with the remote", async () => {
    const user = await libraryRow();
    const before = bridge.count("sources ls");
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await screen.findByRole("dialog", { name: /^Pull from the remote\?$/ });
    expect(
      within(box).getByText("Fast-forward 2 commit(s) from origin/main"),
    ).toBeVisible();
    expect(within(box).getByText("e3bf666 Add notes-one")).toBeVisible();
    expect(bridge.count("library pull")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Pull" }));
    expect(await screen.findByText("Pulled 2 commits")).toBeVisible();
    await finish(user);
    expect(bridge.count("library pull")).toBe(1);
    await library().findByText("Level with origin/main");
    expect(bridge.count("sources ls")).toBeGreaterThan(before);
  });

  it("Escape closes the plan, changes nothing and keeps the focus on Pull", async () => {
    const user = await libraryRow();
    const pull = library().getByRole("button", { name: "Pull" });
    await user.click(pull);
    await screen.findByRole("dialog", { name: /^Pull from the remote\?$/ });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(pull).toHaveFocus());
    expect(bridge.count("library pull")).toBe(0);
    expect(library().getByText("2 commits behind, 0 commits ahead")).toBeVisible();
  });

  it("refuses while files are changed here, with the reason and nothing to confirm", async () => {
    const user = await libraryRow({ uncommitted: 2 });
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await screen.findByRole("dialog", { name: /^Pull from the remote$/ });
    expect(
      await within(box).findByText(
        /2 uncommitted change\(s\) in .*commit or stash them first/,
      ),
    ).toBeVisible();
    expect(within(box).queryByRole("button", { name: "Pull" })).toBeNull();
    expect(bridge.count("library pull")).toBe(0);
  });
});

describe("sources.library.push", () => {
  it("is blocked by a secret finding: the finding is listed and nothing can be applied", async () => {
    const user = await libraryRow({ ahead: 2, leak: true });
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await screen.findByRole("dialog", {
      name: /^Push the library is blocked$/,
    });
    expect(
      within(within(box).getByRole("list", { name: "Findings" })).getByText(
        "github-token in skills/leaky/SKILL.md:5 (commit 13e1696)",
      ),
    ).toBeVisible();
    expect(within(box).queryByRole("button", { name: "Push" })).toBeNull();
    expect(bridge.count("library push")).toBe(0);
  });

  it("previews the commits, pushes on confirm, and nothing is left to push afterwards", async () => {
    const user = await libraryRow({ ahead: 1, behind: 0 });
    expect(library().getByText("1 commit not pushed")).toBeVisible();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await screen.findByRole("dialog", { name: /^Push the library\?$/ });
    expect(within(box).getByText("Push 1 commit(s) to origin/main")).toBeVisible();
    expect(within(box).getByText("commit 7caa837 Add new-local")).toBeVisible();
    expect(bridge.count("library push")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Push" }));
    expect(await screen.findByText("Pushed to the remote")).toBeVisible();
    await finish(user);
    expect(bridge.ran().filter((line) => line.startsWith("library push"))).toEqual([
      "library push --dry-run",
      "library push",
    ]);
    await library().findByText("Nothing changed here");
    expect(library().getByText("Level with origin/main")).toBeVisible();
  });
});
