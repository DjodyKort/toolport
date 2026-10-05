import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen, pick } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  pick: { open: vi.fn(), save: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => pick);
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { SourcesTab } from "./SourcesTab";
import {
  ADDED,
  createSourcesBridge,
  failure,
  REMOVED,
  summaryData,
  wire,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createSourcesBridge();
  wire({ invoke, listen }, bridge);
  pick.open.mockReset();
});

async function open() {
  const user = userEvent.setup();
  render(<SourcesTab />);
  await screen.findByRole("list", { name: "Scanned folders" });
  return user;
}

const dialog = (name: RegExp) => screen.findByRole("dialog", { name });

describe("Sources tab: add a folder to scan", () => {
  it("asks for a folder, previews the change, and writes only after confirming", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Add folder to scan…" }));
    const form = await dialog(/^Add a folder to scan$/);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText("Choose or type a folder")).toBeInTheDocument();
    expect(bridge.ran().some((argv) => argv.startsWith("sources root add"))).toBe(false);
    await user.type(within(form).getByLabelText("Folder"), ADDED);
    await user.click(within(form).getByRole("button", { name: "Preview" }));

    const box = await dialog(/^Add a folder to scan\?$/);
    expect(
      within(box).getByText("add source root /fixture/home/work"),
    ).toBeInTheDocument();
    expect(
      within(box).getByText(/add \/fixture\/home\/work to sourceRoots/),
    ).toBeVisible();
    expect(within(box).getByText(/is not a git checkout/)).toBeInTheDocument();
    expect(within(box).getByText(/toolportctl sources root rm/)).toBeInTheDocument();
    expect(bridge.count(`sources root add ${ADDED} --dry-run`)).toBe(1);
    expect(bridge.count(`sources root add ${ADDED}`)).toBe(0);
    expect(within(box).queryByRole("textbox")).toBeNull();

    await user.click(within(box).getByRole("button", { name: "Add folder" }));
    await screen.findByText("Done");
    expect(bridge.count(`sources root add ${ADDED}`)).toBe(1);
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Scanned folders" })).getAllByRole(
          "listitem",
        ),
      ).toHaveLength(4),
    );
    expect(screen.getByText("work")).toBeInTheDocument();
    expect(bridge.count("sources ls")).toBe(2);
  });

  it("takes the folder from the native picker when there is one", async () => {
    pick.open.mockResolvedValue(ADDED);
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Add folder to scan…" }));
    const form = await dialog(/^Add a folder to scan$/);
    await user.click(within(form).getByRole("button", { name: /^Choose folder/ }));
    await waitFor(() => expect(within(form).getByLabelText("Folder")).toHaveValue(ADDED));
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await dialog(/^Add a folder to scan\?$/);
    expect(bridge.count(`sources root add ${ADDED} --dry-run`)).toBe(1);
  });

  it("shows the CLI's words and never applies when the preview fails", async () => {
    bridge.set(
      `sources root add ${ADDED} --dry-run`,
      failure("not_found", `not a directory: ${ADDED}`),
    );
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Add folder to scan…" }));
    const form = await dialog(/^Add a folder to scan$/);
    await user.type(within(form).getByLabelText("Folder"), ADDED);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(
      await screen.findByText(/not a directory: \/fixture\/home\/work/),
    ).toBeVisible();
    expect(bridge.count(`sources root add ${ADDED}`)).toBe(0);
    expect(screen.queryByRole("button", { name: "Add folder" })).toBeNull();
  });

  it("cancels the form without running anything", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Add folder to scan…" }));
    const form = await dialog(/^Add a folder to scan$/);
    await user.click(within(form).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.ran().some((argv) => argv.startsWith("sources root add"))).toBe(false);
  });
});

describe("Sources tab: stop scanning a folder", () => {
  it("previews first and makes you type the folder's name before removing it", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Stop scanning dups" }));
    const box = await dialog(/^Stop scanning dups\?$/);
    expect(within(box).getByText("remove source root /fixture/home/dups")).toBeVisible();
    expect(bridge.count(`sources root rm ${REMOVED} --dry-run`)).toBe(1);
    expect(bridge.count(`sources root rm ${REMOVED}`)).toBe(0);
    const confirm = within(box).getByRole("button", { name: "Stop scanning" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "dup");
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "s");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    await screen.findByText("Done");
    expect(bridge.count(`sources root rm ${REMOVED}`)).toBe(1);
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Scanned folders" })).getAllByRole(
          "listitem",
        ),
      ).toHaveLength(2),
    );
    expect(screen.queryByRole("button", { name: "Stop scanning dups" })).toBeNull();
  });

  it("applies nothing when the dialog is dismissed", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Stop scanning dups" }));
    const box = await dialog(/^Stop scanning dups\?$/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(box).not.toBeInTheDocument());
    expect(bridge.count(`sources root rm ${REMOVED}`)).toBe(0);
  });

  it("refuses a write the registry does not classify", async () => {
    bridge.set("commands", { commands: [], tools: [] });
    const user = userEvent.setup();
    render(<SourcesTab />);
    await screen.findByRole("list", { name: "Scanned folders" });
    await user.click(screen.getByRole("button", { name: "Stop scanning dups" }));
    expect(await screen.findByText(/does not know how safe/)).toBeVisible();
    expect(bridge.count(`sources root rm ${REMOVED} --dry-run`)).toBe(0);
  });
});

describe("Sources tab: nothing secret reaches the screen", () => {
  it("strips a credential from a remote before showing it, and sends none", async () => {
    const canary = "CANARY-remote-token-4d2f";
    const data = summaryData();
    data.sources = data.sources.map((row) =>
      row.id === "library"
        ? {
            ...row,
            status: {
              ...row.status,
              detail: `a second clone of https://deploy:${canary}@git.example.test/acme/skills.git`,
            },
            warnings: [`remote is https://deploy:${canary}@git.example.test/acme/skills`],
          }
        : row,
    );
    bridge.set("sources ls", data);
    const user = userEvent.setup();
    render(<SourcesTab />);
    await screen.findByRole("list", { name: "Sources" });
    await user.click(
      within(screen.getByRole("list", { name: "Sources" })).getByRole("button", {
        name: /ai-skills/,
      }),
    );
    expect(
      screen.getByText(/a second clone of https:\/\/git\.example\.test/),
    ).toBeVisible();
    expect(document.body.textContent).not.toContain(canary);
    expect(JSON.stringify(invoke.mock.calls)).not.toContain(canary);
  });
});
