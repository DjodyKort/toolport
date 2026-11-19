import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import AgentsStyles from "./AgentsStyles";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const ranExactly = (...argv: string[]) =>
  ranArgv().some(
    (run) => run.length === argv.length && argv.every((a, i) => run[i] === a),
  );

/** The JSON a `mcp call` received on stdin, which is where tool arguments travel. */
function stdinOf(tool: string): Record<string, unknown> | null {
  const call = invokeLog.find(
    (entry) =>
      entry.command === "plus_ctl" && (entry.args as { argv: string[] }).argv[2] === tool,
  );
  const text = (call?.args as { stdinSecret?: string } | undefined)?.stdinSecret;
  return text ? (JSON.parse(text) as Record<string, unknown>) : null;
}

beforeEach(() => {
  invokeLog.length = 0;
});

describe("Library screen, editing agents and styles", () => {
  it("test01_library_agents_edit_and_library_styles_edit_are_built_and_name_this_file", () => {
    for (const id of ["library.agents-edit", "library.styles-edit"]) {
      expect(guiParity.actions[id], id).toMatchObject({
        status: "built",
        route: "library",
        test: "src/plus/library/AgentsStylesEditing.test.tsx",
      });
    }
  });

  it("test02_a_new_agent_is_previewed_then_scaffolded_in_the_library", async () => {
    resetPlusFixture({});
    render(<AgentsStyles />);
    await userEvent.click(await screen.findByRole("button", { name: /New agent/ }));
    const ask = await screen.findByRole("dialog");
    await userEvent.type(within(ask).getByLabelText("Name"), "doc-reader");
    await userEvent.click(within(ask).getByRole("button", { name: "Preview" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/agents\/doc-reader\/AGENT\.md/),
    ).toBeVisible();
    expect(ranExactly("agents", "add", "doc-reader", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(ranExactly("agents", "add", "doc-reader")).toBe(true));
    expect(await screen.findByText("doc-reader")).toBeVisible();
    expect(await screen.findByText("New since the last sync")).toBeVisible();
  });

  it("test03_the_body_of_an_agent_is_read_edited_and_saved_without_a_command_line", async () => {
    resetPlusFixture({});
    render(<AgentsStyles />);
    await userEvent.click(await screen.findByRole("button", { name: "Edit body" }));
    const dialog = await screen.findByRole("dialog");
    const box = await within(dialog).findByLabelText("Body");
    expect(box).toHaveValue("You read a repository and report what you find.\n");
    expect(within(dialog).getByRole("button", { name: "Review changes" })).toBeDisabled();
    await userEvent.clear(box);
    await userEvent.type(box, "Read first, then report.");
    expect(within(dialog).queryByRole("button", { name: "Save body" })).toBeNull();
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    expect(await within(dialog).findByLabelText("Changes")).toBeVisible();
    expect(stdinOf("agents_edit_body")).toBeNull();
    await userEvent.click(within(dialog).getByRole("button", { name: "Save body" }));
    await waitFor(() =>
      expect(stdinOf("agents_edit_body")).toEqual({
        name: "code-scout",
        new_body: "Read first, then report.",
        confirm: true,
      }),
    );
    expect(
      ranArgv().some((argv) => argv.join(" ").includes("Read first, then report.")),
    ).toBe(false);
    expect(await screen.findByText("Edited since the last sync")).toBeVisible();
  });

  it("test04_removing_the_synced_copies_previews_the_files_and_keeps_the_library", async () => {
    resetPlusFixture({});
    render(<AgentsStyles />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Remove synced copies…" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/\.claude\/agents\/code-scout\.md/),
    ).toBeVisible();
    expect(ranExactly("agents", "clean", "--dry-run")).toBe(true);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Remove synced copies" }),
    );
    await waitFor(() => expect(ranExactly("agents", "clean")).toBe(true));
    expect(await screen.findByText("code-scout")).toBeVisible();
  });

  it("test05_a_style_is_created_edited_and_its_synced_copies_removed_the_same_way", async () => {
    resetPlusFixture({ library: { styles: true } });
    render(<AgentsStyles />);
    const styles = await screen.findByRole("region", { name: "Output styles" });
    await userEvent.click(within(styles).getByRole("button", { name: "Edit body" }));
    const dialog = await screen.findByRole("dialog");
    const box = await within(dialog).findByLabelText("Body");
    await userEvent.clear(box);
    await userEvent.type(box, "Two sentences.");
    await userEvent.click(within(dialog).getByRole("button", { name: "Review changes" }));
    await userEvent.click(
      await within(dialog).findByRole("button", { name: "Save body" }),
    );
    await waitFor(() =>
      expect(stdinOf("styles_edit_body")).toMatchObject({
        name: "plain",
        new_body: "Two sentences.",
      }),
    );
    await userEvent.click(
      within(await screen.findByRole("region", { name: "Output styles" })).getByRole(
        "button",
        { name: /New style/ },
      ),
    );
    const ask = await screen.findByRole("dialog");
    await userEvent.type(within(ask).getByLabelText("Name"), "terse");
    await userEvent.click(within(ask).getByRole("button", { name: "Preview" }));
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Create",
      }),
    );
    await waitFor(() => expect(ranExactly("styles", "add", "terse")).toBe(true));
    await userEvent.click(
      within(await screen.findByRole("region", { name: "Output styles" })).getByRole(
        "button",
        { name: "Remove synced copies…" },
      ),
    );
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Remove synced copies",
      }),
    );
    await waitFor(() => expect(ranExactly("styles", "clean")).toBe(true));
  });
});
