import { describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Tabs, type TabItem } from "./Tabs";

const items: TabItem[] = [
  { id: "skills", label: "Skills" },
  { id: "agents", label: "Agents", count: 4 },
  { id: "styles", label: "Styles" },
];

function Harness({
  initial = "skills",
  onChange,
  orientation,
}: {
  initial?: string;
  onChange?: (id: string) => void;
  orientation?: "horizontal" | "vertical";
}) {
  const [value, setValue] = useState(initial);
  return (
    <Tabs
      items={items}
      value={value}
      label="Library"
      orientation={orientation}
      onValueChange={(id) => {
        onChange?.(id);
        setValue(id);
      }}
    >
      <p>Panel of {value}</p>
    </Tabs>
  );
}

describe("Tabs", () => {
  it("exposes a tab list, the selected tab and a panel named by it", () => {
    render(<Harness />);
    const list = screen.getByRole("tablist", { name: "Library" });
    expect(list).toHaveAttribute("aria-orientation", "horizontal");
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((tab) => tab.getAttribute("aria-selected"))).toEqual([
      "true",
      "false",
      "false",
    ]);
    expect(screen.getByRole("tabpanel", { name: "Skills" })).toHaveTextContent(
      "Panel of skills",
    );
    expect(tabs[0]).toHaveAttribute("aria-controls", screen.getByRole("tabpanel").id);
    expect(screen.getByRole("tab", { name: /Agents/ })).toHaveTextContent("4");
  });

  it("keeps one tab stop: only the selected tab is in the tab order", () => {
    render(<Harness initial="agents" />);
    expect(screen.getAllByRole("tab").map((tab) => tab.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("selects and focuses the next tab on the arrow keys, wrapping at both ends", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Harness onChange={onChange} />);
    const [skills, agents, styles] = screen.getAllByRole("tab");
    skills.focus();
    await user.keyboard("{ArrowRight}");
    expect(agents).toHaveFocus();
    expect(agents).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{End}");
    expect(styles).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(skills).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(styles).toHaveFocus();
    await user.keyboard("{Home}");
    expect(skills).toHaveFocus();
    expect(onChange.mock.calls.map(([id]) => id)).toEqual([
      "agents",
      "styles",
      "skills",
      "styles",
      "skills",
    ]);
    expect(screen.getByRole("tabpanel", { name: "Skills" })).toBeInTheDocument();
  });

  it("selects a tab on click and ignores keys it does not own", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Harness onChange={onChange} />);
    await user.click(screen.getByRole("tab", { name: "Styles" }));
    expect(onChange).toHaveBeenCalledWith("styles");
    screen.getByRole("tab", { name: "Styles" }).focus();
    await user.keyboard("{ArrowDown}x");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("moves with Up and Down when vertical", async () => {
    const user = userEvent.setup();
    render(<Harness orientation="vertical" />);
    screen.getAllByRole("tab")[0].focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("tab", { name: /Agents/ })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: /Agents/ })).toHaveFocus();
  });

  it("renders only the list when it has no panel content", () => {
    render(<Tabs items={items} value="skills" label="Bare" onValueChange={() => {}} />);
    expect(screen.getAllByRole("tab")).toHaveLength(3);
    expect(screen.queryByRole("tabpanel")).not.toBeInTheDocument();
  });
});
