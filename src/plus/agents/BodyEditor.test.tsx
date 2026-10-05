import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { AgentsTab } from "./AgentsTab";
import { StylesTab } from "./StylesTab";
import { mcpKey } from "./mcpWorld";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const CANARY = "sk-synthetic-canary-0123456789";

async function openEditor(tab: "agents" | "styles", name: string) {
  const user = userEvent.setup();
  render(tab === "agents" ? <AgentsTab /> : <StylesTab />);
  await user.click(await screen.findByRole("button", { name: `Edit body of ${name}` }));
  return user;
}

const editor = () => screen.findByRole("dialog");

describe("Agents tab: Edit body", () => {
  it("reads the file, previews the change, and writes it only after Save", async () => {
    const user = await openEditor("agents", "scout");
    const box = await editor();
    const text = await within(box).findByRole("textbox", { name: "Body of scout" });
    expect(text).toHaveValue("Body of scout\n");
    expect(within(box).getByRole("button", { name: "Review changes" })).toBeDisabled();
    expect(bridge.count(mcpKey("agents_get"))).toBe(1);

    await user.clear(text);
    await user.type(text, "Look first, then answer.");
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    expect(await within(box).findByText("Save agent scout?")).toBeVisible();
    expect(within(box).getByText("Edit agent scout")).toBeVisible();
    expect(within(box).getByText(/2 line\(s\) removed, 1 added/)).toBeVisible();
    expect(bridge.count(mcpKey("agents_edit_body"))).toBe(0);

    await user.click(within(box).getByRole("button", { name: "Save" }));
    expect(await within(box).findByText(/Saved agent scout/)).toBeVisible();
    expect(bridge.stdins(mcpKey("agents_edit_body"))).toEqual([
      { name: "scout", new_body: "Look first, then answer.", confirm: true },
    ]);
    await user.click(within(box).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await user.click(screen.getByRole("button", { name: "Edit body of scout" }));
    expect(
      await within(await editor()).findByRole("textbox", { name: "Body of scout" }),
    ).toHaveValue("Look first, then answer.");
  });

  it("shows a refusal and does not say it saved", async () => {
    bridge.set(
      mcpKey("agents_edit_body"),
      failure(
        "refused",
        "Refused: agents_edit_body (tier 3). Pass confirm=true to proceed.",
      ),
    );
    const user = await openEditor("agents", "scout");
    const box = await editor();
    await user.type(
      await within(box).findByRole("textbox", { name: "Body of scout" }),
      "x",
    );
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    await user.click(await within(box).findByRole("button", { name: "Save" }));
    expect(await within(box).findByRole("alert")).toHaveTextContent(
      /Refused: agents_edit_body/,
    );
    expect(within(box).queryByText(/^Saved agent/)).toBeNull();
    expect(within(box).getByRole("button", { name: "Review changes" })).toBeEnabled();
  });

  it("says the file could not be read, with Retry", async () => {
    bridge.set(mcpKey("agents_get"), failure("not_found", "agent not found: scout"));
    await openEditor("agents", "scout");
    const box = await editor();
    expect(await within(box).findByText(/agent not found: scout/)).toBeVisible();
    bridge.set(mcpKey("agents_get"), undefined as never);
    expect(within(box).getByRole("button", { name: /Retry/ })).toBeEnabled();
    expect(within(box).getByRole("button", { name: "Review changes" })).toBeDisabled();
  });

  it("is a loading state while the file is read", async () => {
    let release: () => void = () => {};
    const held = new Promise<void>((resolve) => (release = resolve));
    const reply = bridge.get(mcpKey("agents_get")) as (
      a: string[],
      s?: string,
    ) => unknown;
    bridge.set(mcpKey("agents_get"), async (argv: string[], stdin?: string) => {
      await held;
      return reply(argv, stdin);
    });
    await openEditor("agents", "scout");
    const box = await editor();
    expect(within(box).queryByRole("textbox")).toBeNull();
    release();
    expect(
      await within(box).findByRole("textbox", { name: "Body of scout" }),
    ).toBeVisible();
  });

  it("cancels with Escape, writes nothing and gives the focus back", async () => {
    const user = await openEditor("agents", "scout");
    const box = await editor();
    await user.type(
      await within(box).findByRole("textbox", { name: "Body of scout" }),
      "x",
    );
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count(mcpKey("agents_edit_body"))).toBe(0);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Edit body of scout" })).toHaveFocus(),
    );
  });

  it("keeps the text of the file off the command line and out of the log", async () => {
    const logs = ["log", "info", "warn", "error", "debug"].map((level) =>
      vi.spyOn(console, level as "log").mockImplementation(() => {}),
    );
    const user = await openEditor("agents", "scout");
    const box = await editor();
    await user.type(
      await within(box).findByRole("textbox", { name: "Body of scout" }),
      CANARY,
    );
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    await user.click(await within(box).findByRole("button", { name: "Save" }));
    await within(box).findByText(/Saved agent scout/);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    expect(bridge.stdins(mcpKey("agents_edit_body"))[0]?.new_body).toContain(CANARY);
    for (const spy of logs) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(CANARY);
      spy.mockRestore();
    }
  });
});

describe("Styles tab: Edit body", () => {
  it("writes the style body with styles_edit_body", async () => {
    bridge.set("styles ls", {
      active: [],
      discoveryWarnings: [],
      lockfilePresent: false,
      repo: "/fixture/skills-repo",
      styles: [
        {
          name: "plain",
          description: "A synthetic plain style",
          keepCodingInstructions: true,
          path: "/fixture/skills-repo/styles/plain/STYLE.md",
          clientsSynced: [],
          synced: false,
        },
      ],
    });
    bridge.set("styles status", {
      applyRemove: [],
      lockfilePresent: false,
      native: [],
      repo: "/fixture/skills-repo",
    });
    const user = await openEditor("styles", "plain");
    const box = await editor();
    const text = await within(box).findByRole("textbox", { name: "Body of plain" });
    await user.type(text, "Short.");
    await user.click(within(box).getByRole("button", { name: "Review changes" }));
    expect(await within(box).findByText("Edit style plain")).toBeVisible();
    await user.click(within(box).getByRole("button", { name: "Save" }));
    expect(await within(box).findByText(/Saved style plain/)).toBeVisible();
    expect(bridge.stdins(mcpKey("styles_edit_body"))).toEqual([
      { name: "plain", new_body: "Body of plain\nShort.", confirm: true },
    ]);
  });
});

describe("Converted for", () => {
  it("lists what each client gets for agents and for styles", async () => {
    render(<AgentsTab />);
    expect(await screen.findByRole("list", { name: "Converters" })).toHaveTextContent(
      /claude-code.*codex-cli.*cursor/,
    );
  });

  it("splits the styles into the native toggle and the always-on rule", async () => {
    render(<StylesTab />);
    expect(
      await screen.findByRole("list", { name: "Native-toggle clients" }),
    ).toHaveTextContent("claude-code");
    expect(screen.getByRole("list", { name: "Always-on clients" })).toHaveTextContent(
      "zed",
    );
  });

  it("says the converters could not be listed and offers Retry", async () => {
    bridge.set(
      mcpKey("agents_list_transpilers"),
      failure("internal", "no converter table"),
    );
    render(<AgentsTab />);
    expect(
      await screen.findByText(/could not be listed: no converter table/),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
  });
});
