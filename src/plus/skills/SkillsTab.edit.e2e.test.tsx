import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { mcpKey } from "../agents/mcpWorld";
import { openLibrary } from "./e2e";
import { createBridge, failure, wire, type Bridge } from "./testkit";

/** The detail of a skill: Open editor reads the file with skills_get and writes with
 * skills_edit_body and skills_edit_frontmatter, Delete from library calls skills_delete after
 * the name is typed, and Converted for lists skills_list_transpilers. Each test is named by the
 * parity action it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
});

const CANARY = "sk-synthetic-canary-0123456789";
const list = () => screen.getByRole("list", { name: "Skills" });
const rowButton = (name: string) =>
  within(list())
    .getAllByRole("button")
    .find((b) => within(b).queryByText(name, { selector: "b" }))!;

async function openDetail(name: string) {
  const user = await openLibrary();
  await user.click(rowButton(name));
  await screen.findByRole("region", { name: `Skill ${name}` });
  return user;
}

describe("skills.edit-body: Open editor", () => {
  it("reads the file, previews, writes on Save, and the next read has the new body", async () => {
    const user = await openDetail("api-review");
    await user.click(screen.getByRole("button", { name: "Open editor for api-review" }));
    const box = await screen.findByRole("dialog");
    const text = await within(box).findByRole("textbox", { name: "Body of api-review" });
    expect(text).toHaveValue("Body of api-review\n");
    await user.type(text, "Second line.");
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    expect(await within(box).findByText("Save skill api-review?")).toBeVisible();
    expect(bridge.count(mcpKey("skills_edit_body"))).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Save" }));
    expect(await within(box).findByText(/Saved skill api-review/)).toBeVisible();
    expect(bridge.stdins(mcpKey("skills_edit_body"))).toEqual([
      { name: "api-review", new_body: "Body of api-review\nSecond line.", confirm: true },
    ]);
    expect(bridge.count(mcpKey("skills_edit_frontmatter"))).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await user.click(screen.getByRole("button", { name: "Open editor for api-review" }));
    expect(
      await within(await screen.findByRole("dialog")).findByRole("textbox", {
        name: "Body of api-review",
      }),
    ).toHaveValue("Body of api-review\nSecond line.");
  });

  it("keeps the body off the command line and out of the log, and Escape writes nothing", async () => {
    const logs = ["log", "info", "warn", "error"].map((level) =>
      vi.spyOn(console, level as "log").mockImplementation(() => {}),
    );
    const user = await openDetail("api-review");
    await user.click(screen.getByRole("button", { name: "Open editor for api-review" }));
    const box = await screen.findByRole("dialog");
    await user.type(
      await within(box).findByRole("textbox", { name: "Body of api-review" }),
      CANARY,
    );
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count(mcpKey("skills_edit_body"))).toBe(0);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    for (const spy of logs) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(CANARY);
      spy.mockRestore();
    }
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Open editor for api-review" }),
      ).toHaveFocus(),
    );
  });

  it("shows a refused write and does not claim it saved", async () => {
    bridge.set(
      mcpKey("skills_edit_body"),
      failure(
        "refused",
        "Refused: skills_edit_body (tier 3). Pass confirm=true to proceed.",
      ),
    );
    const user = await openDetail("api-review");
    await user.click(screen.getByRole("button", { name: "Open editor for api-review" }));
    const box = await screen.findByRole("dialog");
    await user.type(
      await within(box).findByRole("textbox", { name: "Body of api-review" }),
      "x",
    );
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    await user.click(await within(box).findByRole("button", { name: "Save" }));
    expect(await within(box).findByRole("alert")).toHaveTextContent(/Refused/);
    expect(within(box).queryByText(/^Saved skill/)).toBeNull();
  });
});

describe("skills.edit-frontmatter: the description of a skill", () => {
  it("is patched with skills_edit_frontmatter and shown in the preview", async () => {
    const user = await openDetail("api-review");
    await user.click(screen.getByRole("button", { name: "Open editor for api-review" }));
    const box = await screen.findByRole("dialog");
    const field = await within(box).findByRole("textbox", { name: "Description" });
    expect(field).toHaveValue("A synthetic skill api-review");
    await user.clear(field);
    await user.type(field, "Reviews an API change");
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    expect(
      await within(box).findByText("Change the description of skill api-review"),
    ).toBeVisible();
    await user.click(within(box).getByRole("button", { name: "Save" }));
    expect(await within(box).findByText(/Saved skill api-review/)).toBeVisible();
    expect(bridge.stdins(mcpKey("skills_edit_frontmatter"))).toEqual([
      {
        name: "api-review",
        patch: { description: "Reviews an API change" },
        confirm: true,
      },
    ]);
    expect(bridge.count(mcpKey("skills_edit_body"))).toBe(0);
  });
});

describe("skills.delete: Delete from library", () => {
  it("needs the typed name, then skills_delete runs and the skill cannot be read any more", async () => {
    const user = await openDetail("api-review");
    await user.click(
      screen.getByRole("button", { name: "Delete api-review from the library" }),
    );
    const box = await screen.findByRole("dialog", { name: "Delete skill api-review?" });
    expect(within(box).getByText(/Uninstall removes them too/)).toBeVisible();
    const confirm = within(box).getByRole("button", { name: "Delete" });
    expect(confirm).toBeDisabled();
    expect(bridge.count(mcpKey("skills_delete"))).toBe(0);
    await user.type(within(box).getByRole("textbox"), "api-review");
    await user.click(confirm);
    expect(await screen.findByText(/Deleted api-review\. Removed/)).toBeVisible();
    expect(bridge.stdins(mcpKey("skills_delete"))).toEqual([
      { name: "api-review", confirm: true },
    ]);
    await user.click(screen.getByRole("button", { name: "Done" }));
    const reply = bridge.get(mcpKey("skills_get")) as (
      a: string[],
      s?: string,
    ) => unknown;
    expect(reply([], JSON.stringify({ name: "api-review" }))).toMatchObject({
      code: "not_found",
    });
  });

  it("says nothing was deleted when the tool fails", async () => {
    bridge.set(
      mcpKey("skills_delete"),
      failure("not_found", "skill not found: api-review"),
    );
    const user = await openDetail("api-review");
    await user.click(
      screen.getByRole("button", { name: "Delete api-review from the library" }),
    );
    const box = await screen.findByRole("dialog", { name: "Delete skill api-review?" });
    await user.type(within(box).getByRole("textbox"), "api-review");
    await user.click(within(box).getByRole("button", { name: "Delete" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Nothing was deleted: skill not found: api-review",
    );
  });
});

describe("skills.transpilers: Converted for", () => {
  it("lists the clients skills are converted for", async () => {
    await openLibrary();
    expect(await screen.findByRole("list", { name: "Converters" })).toHaveTextContent(
      /aider.*claude-code.*cursor.*zed/,
    );
  });
});
