import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
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

const undoOf = () =>
  (
    vi.mocked(toast.message).mock.calls.at(-1)?.[1] as unknown as {
      action: { label: string; onClick: () => void };
    }
  ).action;

async function perClient() {
  return screen.findByRole("region", { name: "Style per client" });
}

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, the style of each client", () => {
  it("test01_library_styles_apply_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.styles-apply"]).toMatchObject({
      status: "built",
      route: "library",
      test: "src/plus/library/StyleClients.test.tsx",
    });
    expect(guiParity.commands["styles apply"]).toMatchObject({ route: "library" });
    expect(guiParity.tools.styles_apply).toMatchObject({ route: "library" });
  });

  it("test02_every_client_without_a_native_switch_shows_the_style_it_has", async () => {
    resetPlusFixture({ library: { applied: true } });
    render(<AgentsStyles />);
    const region = await perClient();
    expect(await within(region).findByText("Cursor")).toBeVisible();
    const cursor = within(region).getByText("Cursor").closest("li") as HTMLElement;
    expect(within(cursor).getByText("plain")).toBeVisible();
    const aider = within(region).getByText("Aider").closest("li") as HTMLElement;
    expect(within(aider).getByText("No style")).toBeVisible();
    expect(region).toHaveTextContent("Claude Code");
    expect(region).toHaveTextContent("User level");
  });

  it("test03_a_style_is_previewed_for_the_chosen_client_before_it_is_written", async () => {
    resetPlusFixture({ library: { styles: true } });
    render(<AgentsStyles />);
    const region = await perClient();
    await userEvent.click(
      await within(region).findByRole("button", { name: "Change the style of Cursor" }),
    );
    const choose = await screen.findByRole("dialog");
    expect(within(choose).getByLabelText("Style")).toHaveValue("plain");
    expect(within(choose).getByRole("checkbox", { name: /Cursor/ })).toBeChecked();
    expect(within(choose).getByRole("checkbox", { name: /Aider/ })).not.toBeChecked();
    await userEvent.click(within(choose).getByRole("button", { name: "Preview" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/\.cursor\/rules\/mcpm-output-style\/RULE\.md/),
    ).toBeVisible();
    expect(
      ranExactly("styles", "apply", "plain", "--client", "cursor", "--dry-run"),
    ).toBe(true);
    expect(ranExactly("styles", "apply", "plain", "--client", "cursor")).toBe(false);

    await userEvent.click(within(dialog).getByRole("button", { name: "Apply plain" }));
    await waitFor(() =>
      expect(ranExactly("styles", "apply", "plain", "--client", "cursor")).toBe(true),
    );
    const row = (await within(await perClient()).findByText("Cursor")).closest(
      "li",
    ) as HTMLElement;
    expect(await within(row).findByText("plain")).toBeVisible();
  });

  it("test04_undo_takes_the_style_off_a_client_that_had_none", async () => {
    resetPlusFixture({ library: { styles: true } });
    render(<AgentsStyles />);
    await userEvent.click(
      await within(await perClient()).findByRole("button", {
        name: "Change the style of Cursor",
      }),
    );
    await userEvent.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Preview" }),
    );
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Apply plain",
      }),
    );
    await waitFor(() => expect(undoOf().label).toBe("Undo"));
    undoOf().onClick();
    await waitFor(() =>
      expect(ranExactly("styles", "remove", "--client", "cursor")).toBe(true),
    );
    const row = (await within(await perClient()).findByText("Cursor")).closest(
      "li",
    ) as HTMLElement;
    expect(await within(row).findByText("No style")).toBeVisible();
  });

  it("test05_taking_a_style_off_previews_the_files_and_undo_applies_it_again", async () => {
    resetPlusFixture({ library: { applied: true } });
    render(<AgentsStyles />);
    await userEvent.click(
      await within(await perClient()).findByRole("button", {
        name: "Change the style of Cursor",
      }),
    );
    const choose = await screen.findByRole("dialog");
    await userEvent.selectOptions(
      within(choose).getByLabelText("Style"),
      "No style (take the applied one off)",
    );
    await userEvent.click(within(choose).getByRole("button", { name: "Preview" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(/\.cursor\/rules\/mcpm-output-style\/RULE\.md/),
    ).toBeVisible();
    expect(ranExactly("styles", "remove", "--client", "cursor", "--dry-run")).toBe(true);
    expect(ranExactly("styles", "remove", "--client", "cursor")).toBe(false);
    await userEvent.click(within(dialog).getByRole("button", { name: "Take it off" }));
    await waitFor(() =>
      expect(ranExactly("styles", "remove", "--client", "cursor")).toBe(true),
    );
    undoOf().onClick();
    await waitFor(() =>
      expect(ranExactly("styles", "apply", "plain", "--client", "cursor")).toBe(true),
    );
  });

  it("test06_several_clients_are_changed_together_and_the_preview_names_what_it_replaces", async () => {
    resetPlusFixture({ library: { applied: true } });
    render(<AgentsStyles />);
    await userEvent.click(await screen.findByRole("button", { name: /New style/ }));
    const ask = await screen.findByRole("dialog");
    await userEvent.type(within(ask).getByLabelText("Name"), "terse");
    await userEvent.click(within(ask).getByRole("button", { name: "Preview" }));
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Create",
      }),
    );
    await userEvent.click(
      await within(await perClient()).findByRole("button", { name: "Apply a style…" }),
    );
    const choose = await screen.findByRole("dialog");
    await userEvent.selectOptions(within(choose).getByLabelText("Style"), "terse");
    await userEvent.click(within(choose).getByRole("checkbox", { name: /Cursor/ }));
    await userEvent.click(within(choose).getByRole("checkbox", { name: /Zed/ }));
    await userEvent.click(within(choose).getByRole("button", { name: "Preview" }));
    const dialog = await screen.findByRole("dialog");
    const replaced = await within(dialog).findByRole("list", { name: "Replaced" });
    expect(replaced).toHaveTextContent("Replaces plain on cursor");
    expect(replaced).toHaveTextContent("Replaces plain on zed");
    await userEvent.click(within(dialog).getByRole("button", { name: "Apply terse" }));
    await waitFor(() =>
      expect(
        ranExactly("styles", "apply", "terse", "--client", "cursor", "--client", "zed"),
      ).toBe(true),
    );
  });

  it("test07_without_a_style_in_the_library_the_page_says_to_add_one", async () => {
    resetPlusFixture({});
    render(<AgentsStyles />);
    const region = await perClient();
    expect(region).toHaveTextContent("Add an output style first.");
    expect(within(region).getByRole("button", { name: "Apply a style…" })).toBeDisabled();
  });

  it("test08_a_style_status_that_cannot_be_read_is_an_error_with_a_retry_not_an_empty_list", async () => {
    resetPlusFixture({ library: { styleStatusFails: true } });
    render(<AgentsStyles />);
    const region = await perClient();
    expect(await within(region).findByRole("alert")).toHaveTextContent(
      "the styles lock cannot be read",
    );
    expect(within(region).getByRole("button", { name: "Try again" })).toBeVisible();
    expect(within(region).queryByText("Cursor")).toBeNull();
  });
});
