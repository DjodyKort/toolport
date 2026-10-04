import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { openFromLibrary, write } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Styles tab walked the way a person uses it, from "no styles yet" to a synced and
 * applied style and back, against a world where an applied write changes the next read.
 * Each test is named by the parity action it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

const open = async () => {
  const user = await openFromLibrary("Styles");
  await screen.findByText("No output styles yet");
  return user;
};
const group = (name: string) => screen.getByRole("group", { name });

async function create(user: UserEvent) {
  await user.click(screen.getByRole("button", { name: "Create your first style" }));
  const form = await screen.findByRole("dialog");
  await user.type(within(form).getByRole("textbox"), "terse");
  await user.click(within(form).getByRole("button", { name: "Preview" }));
  const box = await screen.findByRole("dialog", { name: /Create style terse/ });
  await within(box).findByText(/Create the style 'terse'/);
  await user.click(within(box).getByRole("button", { name: "Create" }));
  await screen.findByText("Created the style 'terse' from the template");
  await user.click(
    within(screen.getByRole("dialog")).getAllByRole("button", { name: "Close" }).at(-1)!,
  );
  await screen.findByText("Short answers, no preamble");
}

const synced = (user: UserEvent) =>
  write(user, "Sync…", "Sync", { done: "Wrote 1 style to 2 native clients" });
const applied = (user: UserEvent) =>
  write(user, "Apply terse to other clients", "Apply", {
    done: "Applied 'terse' as an always-on rule in 4 clients",
  });

const cell = (table: HTMLElement, client: string) =>
  within(table).getByText(client).closest("tr")!;

describe("Styles tab, end to end", () => {
  it("styles.list: starts empty with one action and nothing else to press", async () => {
    await open();
    expect(screen.getByRole("button", { name: "Create your first style" })).toBeVisible();
    for (const name of [/Sync/, /Remove/, /Clean/, /Apply/, /New style/]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    expect(screen.queryByRole("table")).toBeNull();
    const styles = new Set(bridge.ran().filter((line) => line.startsWith("styles")));
    expect([...styles].sort()).toEqual([
      "styles diff",
      "styles lint",
      "styles ls",
      "styles status",
    ]);
  });

  it("styles.add: previews the template file, creates the style, and the list shows it", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Create your first style" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByRole("textbox"), "terse");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Create style terse/ });
    expect(
      await within(box).findByText("/fixture/skills-repo/styles/terse/STYLE.md"),
    ).toBeVisible();
    expect(bridge.count("styles add terse")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText("Created the style 'terse' from the template");
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findByText("Short answers, no preamble")).toBeVisible();
    expect(screen.getByText("Not synced yet")).toBeVisible();
    expect(screen.getByText("No client")).toBeVisible();
    expect(screen.getByRole("button", { name: "New style…" })).toBeVisible();
    expect(bridge.count("styles add terse")).toBe(1);
  });

  it("styles.lint, styles.diff: both checks read, and the diff follows the sync", async () => {
    const user = await open();
    await create(user);
    expect(
      await within(group("Lint")).findByText("No problems in your styles"),
    ).toBeVisible();
    const diff = () => group("Changes since last sync");
    expect(
      await within(diff()).findByText("Never synced: every style is new"),
    ).toBeVisible();
    await synced(user);
    expect(
      await within(diff()).findByText("No changes since the last sync"),
    ).toBeVisible();
  });

  it("styles.sync: previews the native clients, writes, and the card shows where it is synced", async () => {
    const user = await open();
    await create(user);
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const box = await screen.findByRole("dialog");
    expect(
      await within(box).findByText("Write 1 style to 2 native clients"),
    ).toBeVisible();
    expect(
      within(box).getByText("/fixture/home/.claude/output-styles/terse.md"),
    ).toBeVisible();
    expect(bridge.count("styles sync")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText("Wrote 1 style to 2 native clients");
    expect(bridge.count("styles sync")).toBe(1);
    expect(bridge.ran().some((line) => /--home|--path/.test(line))).toBe(false);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    const synced = await screen.findByText("Synced to");
    const chips = synced.nextElementSibling as HTMLElement;
    expect(within(chips).getByText("Claude Code")).toBeVisible();
    expect(within(chips).getByText("Roo Code modes")).toBeVisible();
  });

  it("styles.status: the per-client table follows sync and apply", async () => {
    const user = await open();
    await create(user);
    expect(await screen.findByText("No client has a style yet.")).toBeVisible();
    await synced(user);
    const table = await screen.findByRole("table");
    expect(cell(table, "Claude Code")).toHaveTextContent("terse");
    expect(cell(table, "Roo Code modes")).toHaveTextContent("terse");
    await applied(user);
    expect(
      await within(await screen.findByRole("table")).findByText("Cursor"),
    ).toBeVisible();
    const after = screen.getByRole("table");
    expect(cell(after, "Cursor")).toHaveTextContent("terse");
    expect(cell(after, "Windsurf")).toHaveTextContent("terse");
  });

  it("styles.apply: previews the always-on rules, applies them, and the style is Active", async () => {
    const user = await open();
    await create(user);
    await user.click(
      screen.getByRole("button", { name: "Apply terse to other clients" }),
    );
    const box = await screen.findByRole("dialog");
    expect(
      await within(box).findByText("Apply 'terse' as an always-on rule in 4 clients"),
    ).toBeVisible();
    expect(
      within(box).getByText("/fixture/home/.cursor/rules/toolport-style/RULE.md"),
    ).toBeVisible();
    expect(bridge.count("styles apply terse")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Apply" }));
    await screen.findByText("Applied 'terse' as an always-on rule in 4 clients");
    expect(bridge.count("styles apply terse")).toBe(1);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findByText("Active")).toBeVisible();
    const always = screen.getByText("Always-on in").nextElementSibling as HTMLElement;
    for (const client of ["Codex CLI", "Cursor", "Gemini CLI", "Windsurf"]) {
      expect(within(always).getByText(client)).toBeVisible();
    }
  });

  it("styles.remove: needs the typed phrase and takes the style off the other clients", async () => {
    const user = await open();
    await create(user);
    await applied(user);
    await screen.findByText("Active");
    await user.click(screen.getByRole("button", { name: "Remove active style…" }));
    const box = await screen.findByRole("dialog");
    expect(
      await within(box).findByText("Remove the active style from 4 clients"),
    ).toBeVisible();
    const confirm = within(box).getByRole("button", { name: "Remove" });
    await user.type(within(box).getByRole("textbox"), "remove styl");
    expect(confirm).toBeDisabled();
    expect(bridge.count("styles remove")).toBe(0);
    await user.type(within(box).getByRole("textbox"), "e");
    await user.click(confirm);
    await screen.findByText("Removed the active style from 4 clients");
    expect(bridge.count("styles remove")).toBe(1);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findByText("No client")).toBeVisible();
    expect(screen.queryByText("Active")).toBeNull();
  });

  it("styles.clean: needs the typed phrase, removes every style file and keeps the style", async () => {
    const user = await open();
    await create(user);
    await synced(user);
    await applied(user);
    await user.click(screen.getByRole("button", { name: "Clean style files…" }));
    const box = await screen.findByRole("dialog");
    expect(await within(box).findByText("Remove 6 style files")).toBeVisible();
    const confirm = within(box).getByRole("button", { name: "Remove" });
    expect(confirm).toBeDisabled();
    expect(bridge.count("styles clean")).toBe(0);
    await user.type(within(box).getByRole("textbox"), "clean styles");
    await user.click(confirm);
    await screen.findByText("Removed 6 style files");
    expect(bridge.count("styles clean")).toBe(1);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findByText("Not synced yet")).toBeVisible();
    expect(screen.getByText("terse", { selector: "b" })).toBeVisible();
    expect(screen.queryByText("Active")).toBeNull();
  });
});
