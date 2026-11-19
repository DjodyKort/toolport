import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { plusFixtureIpc, resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import SourcesTab from "./SourcesTab";

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
  vi.mocked(toast.message).mock.calls.at(-1)?.[1] as unknown as {
    action: { label: string; onClick: () => void };
    duration: number;
  };

async function tapRow(name: string) {
  const taps = await screen.findByRole("region", { name: "Taps" });
  return (await within(taps).findByText(name)).closest("li") as HTMLElement;
}

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Library screen, link a tap to Claude Code", () => {
  it("test01_library_tap_link_is_built_and_names_this_file", () => {
    expect(guiParity.actions["library.tap-link"]).toMatchObject({
      status: "built",
      route: "library",
      test: "src/plus/library/TapLink.test.tsx",
    });
    for (const id of ["skills tap link", "skills tap unlink"]) {
      expect(guiParity.commands[id]).toMatchObject({ route: "library" });
    }
    for (const id of ["skills_tap_link", "skills_tap_unlink"]) {
      expect(guiParity.tools[id]).toMatchObject({ route: "library" });
    }
  });

  it("test02_only_a_tap_that_is_a_plugin_marketplace_can_be_linked", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    const acme = await tapRow("acme-skills");
    expect(within(acme).getByText("Plugin marketplace, not linked")).toBeVisible();
    expect(within(acme).getByRole("button", { name: /Link acme-skills/ })).toBeVisible();
    const plain = await tapRow("community-tools");
    expect(
      within(plain).queryByRole("button", { name: /Link community-tools/ }),
    ).toBeNull();
    expect(within(plain).queryByText(/Linked to Claude Code/)).toBeNull();
  });

  it("test03_linking_shows_the_settings_keys_first_and_writes_after_the_confirm", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    await userEvent.click(
      within(await tapRow("acme-skills")).getByRole("button", {
        name: "Link acme-skills to Claude Code",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText(
        "Link tap acme-skills to Claude Code as marketplace acme",
      ),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("Settings keys")).toHaveTextContent(
      "declare extraKnownMarketplaces.acme",
    );
    expect(dialog).toHaveTextContent(
      /sessions that are already running keep the old state/,
    );
    expect(ranExactly("skills", "tap", "link", "acme-skills", "--dry-run")).toBe(true);
    expect(ranExactly("skills", "tap", "link", "acme-skills")).toBe(false);

    await userEvent.click(within(dialog).getByRole("button", { name: "Link" }));
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "link", "acme-skills")).toBe(true),
    );
    expect(
      await within(await tapRow("acme-skills")).findByText("Linked to Claude Code"),
    ).toBeVisible();
    expect(undoOf().duration).toBe(30000);
  });

  it("test04_undo_unlinks_what_the_message_linked", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    await userEvent.click(
      within(await tapRow("acme-skills")).getByRole("button", {
        name: "Link acme-skills to Claude Code",
      }),
    );
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Link",
      }),
    );
    await waitFor(() => expect(toast.message).toHaveBeenCalled());
    undoOf().action.onClick();
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "unlink", "acme-skills")).toBe(true),
    );
    expect(
      await within(await tapRow("acme-skills")).findByText(
        "Plugin marketplace, not linked",
      ),
    ).toBeVisible();
  });

  it("test05_a_linked_tap_is_unlinked_after_its_preview_and_cannot_be_removed_before", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    await userEvent.click(
      within(await tapRow("acme-skills")).getByRole("button", {
        name: "Link acme-skills to Claude Code",
      }),
    );
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Link",
      }),
    );
    const row = await tapRow("acme-skills");
    await userEvent.click(
      await within(row).findByRole("button", {
        name: "Unlink acme-skills from Claude Code",
      }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("Unlink tap acme-skills from Claude Code"),
    ).toBeVisible();
    expect(ranExactly("skills", "tap", "unlink", "acme-skills", "--dry-run")).toBe(true);
    await userEvent.click(within(dialog).getByRole("button", { name: "Unlink" }));
    await waitFor(() =>
      expect(ranExactly("skills", "tap", "unlink", "acme-skills")).toBe(true),
    );
    expect(
      await within(await tapRow("acme-skills")).findByRole("button", {
        name: "Link acme-skills to Claude Code",
      }),
    ).toBeVisible();
  });

  it("test06_removing_a_linked_tap_is_refused_with_the_command_that_unlinks_it", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    await userEvent.click(
      within(await tapRow("acme-skills")).getByRole("button", {
        name: "Link acme-skills to Claude Code",
      }),
    );
    await userEvent.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Link",
      }),
    );
    await userEvent.click(
      await within(await tapRow("acme-skills")).findByRole("button", {
        name: "More for acme-skills",
      }),
    );
    await userEvent.click(await screen.findByRole("menuitem", { name: /Remove tap/ }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "skills tap unlink acme-skills",
    );
    expect(within(dialog).getByRole("button", { name: "Remove tap" })).toBeDisabled();
  });

  it("test07_a_link_that_is_already_in_place_writes_nothing_and_says_so", async () => {
    resetPlusFixture({});
    render(<SourcesTab />);
    const row = await tapRow("acme-skills");
    const link = within(row).getByRole("button", {
      name: "Link acme-skills to Claude Code",
    });
    plusFixtureIpc("plus_ctl_result", {
      job: plusFixtureIpc("plus_ctl", { argv: ["skills", "tap", "link", "acme-skills"] }),
    });
    invokeLog.length = 0;
    await userEvent.click(link);
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "Already linked: there is nothing to write.",
    );
    expect(within(dialog).getByRole("button", { name: "Link" })).toBeDisabled();
    expect(ranExactly("skills", "tap", "link", "acme-skills")).toBe(false);
  });
});
