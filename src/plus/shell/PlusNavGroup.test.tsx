import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LucideIcon } from "lucide-react";
import type { SourceMessage } from "@/lib/i18n";
import { PlusNavGroup } from "./PlusNavGroup";

function row(
  _icon: LucideIcon,
  label: SourceMessage,
  active: boolean,
  onClick: () => void,
) {
  return (
    <button aria-current={active ? "page" : undefined} onClick={onClick}>
      {label}
    </button>
  );
}

describe("PlusNavGroup", () => {
  it("test01_lists_library_context_and_tasks_under_the_claude_label", () => {
    render(<PlusNavGroup view="servers" onSelectView={vi.fn()} row={row} />);
    const group = screen.getByRole("navigation", { name: "Claude" });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Library",
      "Context",
      "Tasks",
    ]);
  });

  it("test02_marks_the_open_page_and_selects_on_click", async () => {
    const onSelectView = vi.fn();
    render(<PlusNavGroup view="context" onSelectView={onSelectView} row={row} />);
    expect(screen.getByRole("button", { name: "Context" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await userEvent.click(screen.getByRole("button", { name: "Tasks" }));
    expect(onSelectView).toHaveBeenCalledWith("tasks");
  });

  it("test03_hides_the_label_with_upstreams_rail_class", () => {
    render(<PlusNavGroup view="servers" onSelectView={vi.fn()} row={row} />);
    expect(screen.getByText("Claude")).toHaveClass("sidebar-label");
  });
});
