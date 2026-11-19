import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog } from "../servers/testkit";
import ClientContent from "./ClientContent";
import { clientContent } from "./contentPerClient";
import type { AgentStatusData, StatusData, StyleStatusData } from "./data";
import LibraryScreen from "./LibraryScreen";

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

const skills = (extra: Partial<StatusData> = {}): StatusData => ({
  drift: false,
  entries: [
    {
      name: "triage",
      type: "skill",
      clientsSynced: ["claude-code"],
      drifted: false,
      knownToLockfile: true,
    },
    {
      name: "house-voice",
      type: "rule",
      clientsSynced: ["claude-code"],
      drifted: false,
      knownToLockfile: true,
    },
  ],
  lockedCount: 2,
  lockfilePresent: true,
  lockfileSyncedAt: "2026-11-17T07:00:00.000Z",
  outputs: [
    { name: "triage", client: "claude-code", present: true },
    { name: "house-voice", client: "claude-code", present: true },
  ],
  repo: "/lib",
  targetedClients: ["claude-code", "agents-md"],
  ...extra,
});

const agents: AgentStatusData = {
  drift: false,
  lockedCount: 1,
  lockfilePresent: true,
  outputs: [{ name: "code-scout", client: "claude-code", present: true }],
  repo: "/lib",
};

const styles: StyleStatusData = {
  repo: "/lib",
  lockfilePresent: true,
  native: [{ client: "claude-code", name: "Claude Code", styles: ["plain"] }],
  applyRemove: [
    { client: "aider", name: "Aider", active: null },
    { client: "cursor", name: "Cursor", active: "plain" },
  ],
};

beforeEach(() => {
  invokeLog.length = 0;
});

describe("Library screen, what each client holds", () => {
  it("test01_the_content_table_is_part_of_the_built_sync_tab", () => {
    expect(guiParity.actions["library.sync"]).toMatchObject({
      status: "built",
      route: "library",
    });
    expect(guiParity.actions["library.sync"].summary).toContain("what each client holds");
  });

  it("test02_items_are_sorted_into_the_client_and_the_kind_they_are_installed_as", () => {
    const rows = clientContent(skills(), agents, styles);
    expect(rows.map((r) => r.client)).toEqual(["claude-code", "agents-md", "cursor"]);
    const code = rows[0];
    expect(code.items.skills).toEqual([{ name: "triage", present: true }]);
    expect(code.items.rules).toEqual([{ name: "house-voice", present: true }]);
    expect(code.items.agents).toEqual([{ name: "code-scout", present: true }]);
    expect(code.items.styles).toEqual([{ name: "plain", present: true }]);
    expect(rows[1].items.skills).toEqual([]);
    expect(rows[2]).toMatchObject({ label: "Cursor" });
    expect(rows[2].items.styles).toEqual([{ name: "plain", present: true }]);
  });

  it("test03_a_client_with_nothing_still_shows_when_the_sync_targets_it_and_a_style_less_one_does_not", () => {
    const rows = clientContent(skills({ outputs: [] }), null, styles);
    expect(rows.map((r) => r.client)).toEqual(["claude-code", "agents-md", "cursor"]);
    expect(rows.find((r) => r.client === "aider")).toBeUndefined();
  });

  it("test04_the_table_has_one_row_per_client_with_counts_and_the_names_one_click_down", async () => {
    resetPlusFixture({ library: { extras: true } });
    render(<ClientContent skills={skills({ targetedClients: ["claude-code"] })} />);
    const region = await screen.findByRole("region", { name: "Installed per client" });
    const table = await within(region).findByRole("table");
    const row = within(table).getByRole("row", { name: /Claude Code/ });
    expect(
      within(row)
        .getAllByRole("cell")
        .map((c) => c.textContent),
    ).toEqual(["1", "1", "1", "0"]);
    expect(within(table).queryByText("code-scout")).toBeNull();
    await userEvent.click(
      within(row).getByRole("button", { name: /Claude Code, 3 items/ }),
    );
    expect(await within(table).findByText("code-scout")).toBeVisible();
    expect(within(table).getByText("triage")).toBeVisible();
    expect(within(table).getByText("house-voice")).toBeVisible();
  });

  it("test05_a_file_the_lock_says_was_written_but_is_gone_is_flagged", async () => {
    resetPlusFixture({});
    render(
      <ClientContent
        skills={skills({
          outputs: [
            { name: "triage", client: "claude-code", present: false },
            { name: "house-voice", client: "claude-code", present: true },
          ],
        })}
      />,
    );
    const table = await screen.findByRole("table");
    expect(await within(table).findByText("1 missing")).toBeVisible();
    await userEvent.click(within(table).getByRole("button", { name: /Claude Code/ }));
    expect(await within(table).findByText("(file missing)")).toBeVisible();
  });

  it("test06_the_sync_tab_shows_it_below_the_targets_with_the_real_library", async () => {
    resetPlusFixture({ library: { scenario: "done", extras: true } });
    render(<LibraryScreen />);
    const region = await screen.findByRole("region", { name: "Installed per client" });
    const row = await within(region).findByRole("row", { name: /Claude Code/ });
    await waitFor(() =>
      expect(
        within(row)
          .getAllByRole("cell")
          .map((c) => c.textContent),
      ).toEqual(["29", "2", "1", "0"]),
    );
  });

  it("test07_at_390_px_the_table_scrolls_inside_its_frame_and_the_count_columns_stay_tight", async () => {
    resetPlusFixture({});
    render(<ClientContent skills={skills()} />);
    const table = await screen.findByRole("table");
    expect(table.parentElement).toHaveClass("overflow-x-auto");
    for (const head of within(table).getAllByRole("columnheader").slice(1)) {
      expect(head).toHaveClass("px-1.5", "text-xs");
    }
  });
});
