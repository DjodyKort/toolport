import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { StylesTab } from "./StylesTab";
import { createBridge, failure, goldenData, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const plain = {
  clientsSynced: ["claude-code", "roomodes-style"],
  description: "A synthetic plain style",
  keepCodingInstructions: true,
  name: "plain",
  path: "/fixture/skills-repo/styles/plain/STYLE.md",
  synced: true,
};

function withStyles() {
  bridge.set("styles ls", {
    active: [{ client: "cursor", style: "plain" }],
    discoveryWarnings: [],
    lockfilePresent: true,
    repo: "/fixture/skills-repo",
    styles: [plain],
  });
  bridge.set("styles status", {
    applyRemove: [{ client: "cursor", name: "Cursor", active: "plain" }],
    lockfilePresent: true,
    native: [{ client: "claude-code", name: "Claude Code", styles: ["plain"] }],
    repo: "/fixture/skills-repo",
  });
  bridge.set("styles apply plain --dry-run", goldenData("styles-apply.preview"));
  bridge.set("styles apply plain", goldenData("styles-apply.apply"));
  bridge.set("styles remove --dry-run", goldenData("styles-remove.preview"));
  bridge.set("styles remove", goldenData("styles-remove.apply"));
  bridge.set("styles clean --dry-run", goldenData("styles-clean.preview"));
  bridge.set("styles clean", goldenData("styles-clean.apply"));
  bridge.set("styles sync", goldenData("styles-sync.apply"));
  bridge.set("styles sync --dry-run", goldenData("styles-sync.preview"));
}

async function open() {
  const user = userEvent.setup();
  render(<StylesTab />);
  await screen.findByText("Keeps coding instructions");
  return user;
}

describe("Styles tab: empty", () => {
  it("starts empty with one clear action and nothing else to press", async () => {
    render(<StylesTab />);
    expect(await screen.findByText("No output styles yet")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((b) => b.textContent?.trim())).toEqual([
      "Create your first style",
    ]);
    expect(screen.queryByRole("group", { name: "Lint" })).toBeNull();
    expect(
      bridge
        .ran()
        .filter((l) => !l.startsWith("styles "))
        .sort(),
    ).toEqual(["commands"]);
  });

  it("creates the first style: name, preview, confirm, then re-reads the list", async () => {
    const user = userEvent.setup();
    render(<StylesTab />);
    await user.click(
      await screen.findByRole("button", { name: /Create your first style/ }),
    );
    const form = await screen.findByRole("dialog");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText("Give it a name")).toBeInTheDocument();
    await user.type(within(form).getByRole("textbox"), "terse");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog");
    expect(within(box).getByText(/Create the style 'terse'/)).toBeInTheDocument();
    expect(bridge.count("styles add terse")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText(/^Created the style 'terse'/);
    expect(bridge.count("styles add terse")).toBe(1);
    await waitFor(() => expect(bridge.count("styles ls")).toBe(2));
  });

  it("shows an error with Retry when the list cannot be read", async () => {
    bridge.set("styles ls", failure("failed", "Cannot read the skills repository"));
    render(<StylesTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Cannot read the skills repository",
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});

describe("Styles tab: with a style", () => {
  it("shows where the style is synced and where it is always on", async () => {
    withStyles();
    await open();
    expect(screen.getByText("Keeps coding instructions")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    const synced = screen.getByText("Synced to").nextElementSibling as HTMLElement;
    expect(within(synced).getByText("Claude Code")).toBeInTheDocument();
    expect(within(synced).getByText("Roo Code modes")).toBeInTheDocument();
    const always = screen.getByText("Always-on in").nextElementSibling as HTMLElement;
    expect(within(always).getByText("Cursor")).toBeInTheDocument();
    const table = await screen.findByRole("table");
    expect(within(table).getByText("Claude Code").closest("tr")).toHaveTextContent(
      "plain",
    );
    expect(within(table).getAllByText("Cursor")[0].closest("tr")).toHaveTextContent(
      "plain",
    );
    expect(screen.getByRole("button", { name: "Edit body of plain" })).toBeDisabled();
  });

  it("applies a style to the other clients after a preview", async () => {
    withStyles();
    const user = await open();
    await user.click(
      screen.getByRole("button", { name: "Apply plain to other clients" }),
    );
    const box = await screen.findByRole("dialog");
    expect(
      within(box).getByText("Apply 'plain' as an always-on rule in 13 clients"),
    ).toBeInTheDocument();
    expect(bridge.count("styles apply plain")).toBe(0);
    expect(within(box).getByText("toolportctl styles apply plain")).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Apply" }));
    await screen.findByText("Applied 'plain' as an always-on rule in 13 clients");
    expect(bridge.count("styles apply plain")).toBe(1);
  });

  it("removes the active style only after typing the phrase", async () => {
    withStyles();
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Remove active style…" }));
    const box = await screen.findByRole("dialog");
    expect(
      within(box).getByText("Remove the active style from 13 clients"),
    ).toBeInTheDocument();
    const confirm = within(box).getByRole("button", { name: "Remove" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "remove style");
    await user.click(confirm);
    await screen.findByText("Removed the active style from 13 clients");
    expect(bridge.count("styles remove")).toBe(1);
  });

  it("cleans every style file only after typing the phrase", async () => {
    withStyles();
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean style files…" }));
    const box = await screen.findByRole("dialog");
    expect(within(box).getByText("Remove 2 style files")).toBeInTheDocument();
    const confirm = within(box).getByRole("button", { name: "Remove" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "clean styles");
    await user.click(confirm);
    await waitFor(() => expect(bridge.count("styles clean")).toBe(1));
  });

  it("syncs to the native clients after a preview, never with --home", async () => {
    withStyles();
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const box = await screen.findByRole("dialog");
    expect(
      within(box).getByText("Write 1 style to 2 native clients"),
    ).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await waitFor(() => expect(bridge.count("styles sync")).toBe(1));
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
  });

  it("does not apply when the preview fails", async () => {
    withStyles();
    bridge.set(
      "styles apply plain --dry-run",
      failure("failed", "style 'plain' is empty"),
    );
    const user = await open();
    await user.click(
      screen.getByRole("button", { name: "Apply plain to other clients" }),
    );
    expect(await screen.findByText("style 'plain' is empty")).toBeInTheDocument();
    expect(bridge.count("styles apply plain")).toBe(0);
  });

  it("lists style lint messages and the changes since the last sync", async () => {
    withStyles();
    bridge.set("styles lint", {
      discoveryWarnings: [],
      errors: 0,
      infos: 0,
      warnings: 1,
      repo: "r",
      styleCount: 1,
      messages: [{ level: "warning", name: "plain", message: "description is short" }],
    });
    bridge.set("styles diff", {
      clean: false,
      discoveryWarnings: [],
      new: [],
      modified: ["plain"],
      removed: [],
      noLockfile: false,
      unchanged: 0,
      repo: "r",
    });
    await open();
    expect(await screen.findByText("description is short")).toBeInTheDocument();
    expect(await screen.findByText("(modified)")).toBeInTheDocument();
  });
});
