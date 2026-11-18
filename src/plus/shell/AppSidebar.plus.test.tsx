import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ReactNode } from "react";
import type { View } from "@/lib/types";
import type { SettingsSubpage } from "@/lib/settingsPages";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppSidebar } from "@/components/AppSidebar";
import { registerAttentionFeed } from "./attentionFeed";

vi.mock("sonner", () => ({ toast: { info: vi.fn(), success: vi.fn() } }));
vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));
vi.mock("@/lib/api", () => ({
  gatherDiagnostics: vi.fn(),
  getSavingsSummary: vi.fn().mockResolvedValue({
    tokensSaved: 0,
    listLoads: 0,
    peakCatalog: 0,
    sinceTs: 0,
  }),
  listQuarantined: vi.fn().mockResolvedValue([]),
  openDataDir: vi.fn(),
}));
vi.mock("@/lib/openUrl", () => ({ openExternal: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("2.3.6"),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ exit: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("@/lib/updater", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/updater")>()),
  checkForUpdate: vi.fn().mockResolvedValue({ kind: "current" }),
}));
vi.mock("@/components/ShareDialog", () => ({
  ShareDialog: ({ trigger }: { trigger: ReactNode }) => trigger,
}));

function sidebar(props: Partial<React.ComponentProps<typeof AppSidebar>> = {}) {
  return (
    <TooltipProvider>
      <AppSidebar
        registry={null}
        onRegistryChange={vi.fn()}
        view="servers"
        onSelectView={vi.fn()}
        onShortcuts={vi.fn()}
        onReplayOnboarding={vi.fn()}
        {...props}
      />
    </TooltipProvider>
  );
}

beforeEach(() => registerAttentionFeed(null));
afterEach(() => registerAttentionFeed(null));

describe("the sidebar with the Claude group", () => {
  it("test01_keeps_upstreams_six_entries_and_adds_the_group_after_them", () => {
    render(sidebar());
    const views = screen.getByRole("navigation", { name: "Views" });
    expect(
      within(views)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Servers", "Clients", "Skills", "Activity", "Settings", "Sync"]);
    const claude = screen.getByRole("navigation", { name: "Claude" });
    expect(
      within(claude)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Library", "Context", "Tasks", "Spend"]);
    expect(
      views.compareDocumentPosition(claude) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("test01b_no_entry_of_the_sidebar_draws_the_same_icon_as_another", () => {
    render(sidebar());
    const drawings = screen
      .getAllByRole("button")
      .map((b) => b.querySelector("svg.lucide")?.innerHTML)
      .filter(Boolean);
    expect(new Set(drawings).size).toBe(drawings.length);
  });

  it("test02_opens_the_group_pages_and_marks_the_current_one", async () => {
    const onSelectView = vi.fn();
    render(sidebar({ view: "context", onSelectView }));
    expect(screen.getByRole("button", { name: "Context" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await userEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(onSelectView).toHaveBeenCalledWith("library");
  });

  it("test03_settings_lists_toolport_plus_before_help_and_data", () => {
    render(sidebar({ view: "settings", settingsPage: "toolport" }));
    const pages = screen.getByLabelText("Settings pages");
    const names = within(pages)
      .getAllByRole("button")
      .map((b) => b.textContent);
    expect(names).toEqual([
      "General",
      "Tools",
      "Safety",
      "Access",
      "Connections",
      "Toolport+",
      "Help and data",
    ]);
    expect(within(pages).getByRole("button", { name: "Toolport+" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("test04_the_footer_has_no_bell_until_an_attention_feed_exists", async () => {
    render(sidebar());
    expect(screen.queryByRole("button", { name: /Needs attention/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Help" })).toBeInTheDocument();
    const snapshot = {
      checkedAt: null,
      items: [{ id: "a", urgency: "needs-you" as const, title: "x", source: "Servers" }],
    };
    registerAttentionFeed({ snapshot: () => snapshot, subscribe: () => () => {} });
    expect(
      await screen.findByRole("button", { name: "Needs attention, 1" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Help" })).toBeInTheDocument();
  });

  it("test05_manage_in_the_bell_lands_on_the_attention_section_of_settings_toolport", async () => {
    const scrolled = vi.fn();
    Element.prototype.scrollIntoView = scrolled;
    function Shell() {
      const [view, setView] = useState<View>("servers");
      const [page, setPage] = useState<SettingsSubpage>("general");
      return (
        <>
          {sidebar({
            view,
            settingsPage: page,
            onSelectView: (next: View) => {
              if (next === "settings") setPage("general");
              setView(next);
            },
            onSettingsPageChange: setPage,
          })}
          <p data-testid="where">{`${view}/${page}`}</p>
          {view === "settings" && page === "toolport" && (
            <section aria-label="Attention" />
          )}
        </>
      );
    }
    const snapshot = {
      checkedAt: null,
      items: [{ id: "a", urgency: "needs-you" as const, title: "x", source: "Servers" }],
    };
    registerAttentionFeed({ snapshot: () => snapshot, subscribe: () => () => {} });
    render(<Shell />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Needs attention, 1" }),
    );
    await userEvent.click(await screen.findByRole("button", { name: "Manage" }));
    await waitFor(() =>
      expect(screen.getByTestId("where")).toHaveTextContent("settings/toolport"),
    );
    await waitFor(() => expect(scrolled).toHaveBeenCalled());
  });
});
