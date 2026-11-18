import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PROJECT_WEB } from "../fixtures/spendWorld";
import { NO_FILTER } from "./model";
import { FilterBar, SpendHeader } from "./SpendHeader";
import { readWorld } from "./testkit";

function header(filter = NO_FILTER) {
  const { summary } = readWorld();
  const handlers = { onFolder: vi.fn(), onExport: vi.fn(), onPrune: vi.fn() };
  render(
    <SpendHeader
      folders={[...summary.byProject.map((r) => r.key), "Unknown"]}
      filter={filter}
      {...handlers}
    />,
  );
  return handlers;
}

describe("SpendHeader", () => {
  it("test01_the_folder_filter_lists_all_folders_then_each_known_folder", async () => {
    const { onFolder } = header();
    await userEvent.click(screen.getByRole("button", { name: /Folder: All folders/ }));
    const items = screen.getAllByRole("menuitemradio");
    expect(items[0]).toHaveTextContent("All folders");
    expect(items[0]).toHaveAttribute("aria-checked", "true");
    expect(items[1]).toHaveTextContent("~/work/clients/alpha/web-app");
    expect(screen.queryByRole("menuitemradio", { name: /Unknown/ })).toBeNull();
    await userEvent.click(items[1]);
    expect(onFolder).toHaveBeenCalledWith(PROJECT_WEB);
  });

  it("test02_a_chosen_folder_is_the_label_and_all_folders_clears_it", async () => {
    const { onFolder } = header({ ...NO_FILTER, project: PROJECT_WEB });
    const button = screen.getByRole("button", {
      name: "Folder: ~/work/clients/alpha/web-app. Change folder",
    });
    await userEvent.click(button);
    await userEvent.click(screen.getByRole("menuitemradio", { name: "All folders" }));
    expect(onFolder).toHaveBeenCalledWith(null);
  });

  it("test03_export_shows_beside_the_filter_when_wide_and_in_a_more_menu_when_narrow", async () => {
    const { onExport, onPrune } = header();
    const wide = screen.getByRole("button", { name: "Export CSV E" });
    expect(wide).toHaveClass("hidden", "@min-[680px]:inline-flex");
    const more = screen.getByRole("button", { name: "More: Export CSV, Prune now" });
    expect(more).toHaveClass("@min-[680px]:hidden");
    await userEvent.click(more);
    await userEvent.click(screen.getByRole("menuitem", { name: "Prune now…" }));
    expect(onPrune).toHaveBeenCalled();
    await userEvent.click(wide);
    expect(onExport).toHaveBeenCalled();
  });
});

describe("FilterBar", () => {
  it("test04_each_active_filter_has_a_chip_with_a_clear_button_and_the_note", async () => {
    const { summary } = readWorld({}, "--project", PROJECT_WEB);
    const whole = readWorld().summary;
    const onClear = vi.fn();
    render(
      <FilterBar
        filter={{ project: PROJECT_WEB, model: null, task: "Interactive" }}
        summary={summary}
        whole={whole}
        onClear={onClear}
      />,
    );
    expect(screen.getByText("~/work/clients/alpha/web-app")).toBeInTheDocument();
    expect(screen.getByText("Interactive (no task)")).toBeInTheDocument();
    expect(
      screen.getByText(/Filtered\. \d+ of \d+ sessions, 39% of the spend\./),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Clear filter: Interactive (no task)" }),
    );
    expect(onClear).toHaveBeenCalledWith("task");
  });

  it("test05_without_a_filter_nothing_is_drawn", () => {
    const { container } = render(
      <FilterBar filter={NO_FILTER} summary={null} whole={null} onClear={() => {}} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
