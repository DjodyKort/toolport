import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FolderProfiles } from "./FolderProfiles";
import type { FolderProfiles as Data } from "./api";

const base: Data = {
  enabled: false,
  mappings: [{ path: "/proj/work", profile: "Work" }],
  folders: [
    {
      root: "/proj/work/app",
      applies: false,
      profile: null,
      wouldApply: "Work",
      rule: "/proj/work",
      reason: "mapping /proj/work matches but folder profiles are disabled",
      launchProfile: null,
      tokens: 120,
    },
  ],
};

describe("FolderProfiles", () => {
  it("shows the inactive profile and toggles", () => {
    const onToggle = vi.fn();
    render(<FolderProfiles data={base} onToggle={onToggle} />);
    const rows = within(
      screen.getByRole("list", { name: "Active profile per folder" }),
    ).getAllByRole("listitem");
    expect(rows[0].textContent).toContain("Work (inactive)");
    expect(rows[0].textContent).toContain("~120 tokens");
    fireEvent.click(screen.getByRole("checkbox"));
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it("shows the active profile when enabled", () => {
    const data = {
      ...base,
      enabled: true,
      folders: [{ ...base.folders[0], applies: true, profile: "Work" }],
    };
    render(<FolderProfiles data={data} onToggle={() => {}} />);
    expect(screen.getByRole("listitem").textContent).toContain("Work");
    expect(screen.getByRole("listitem").textContent).not.toContain("inactive");
  });
});
