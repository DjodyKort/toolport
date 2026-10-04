import { readFileSync } from "node:fs";
import { join } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PlanPreview } from "./PlanPreview";
import { planOf, resultOf, toShown, type PlanV1 } from "./plan";

vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));

const golden = (name: string) =>
  JSON.parse(
    readFileSync(
      join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes", name),
      "utf8",
    ),
  ).envelope.data as unknown;

const plan: PlanV1 = {
  summary: "Apply the profile corp-tools-off to /work/app",
  steps: [
    { op: "create", path: "/work/app/CLAUDE.local.md", detail: "Managed block written" },
    {
      op: "merge",
      path: "/work/app/.claude/settings.local.json",
      detail: "Two keys changed",
      keys: ["skillOverrides", "enabledPlugins"],
      diff: {
        before: '"enabledPlugins": {"corp-tools": true}',
        after: '"enabledPlugins": {}',
      },
    },
    { op: "delete", detail: "Old backup removed" },
    { op: "exec", detail: "Restart the gateway" },
    { op: "note", detail: "Other keys stay as they are" },
  ],
  effects: { tokens: { before: 12600, after: 3950, basis: "measured" } },
  warnings: ["Claude Code has to be restarted in that folder"],
  undo: "toolportctl context bundle undo --cwd /work/app",
};

beforeEach(() => {
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
    configurable: true,
  });
});

describe("PlanPreview with a PlanV1", () => {
  it("lists the summary and every step with the operation named for a screen reader", () => {
    render(<PlanPreview data={{ plan }} />);
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByText(plan.summary)).toBeInTheDocument();
    const steps = within(screen.getByRole("list", { name: "Changes" })).getAllByRole(
      "listitem",
    );
    expect(steps).toHaveLength(5);
    expect(steps[0]).toHaveTextContent("Create: Managed block written");
    expect(steps[0]).toHaveTextContent("/work/app/CLAUDE.local.md");
    expect(steps[1]).toHaveTextContent("Merge: Two keys changed");
    expect(steps[1]).toHaveTextContent("skillOverrides");
    expect(steps[2]).toHaveTextContent("Delete: Old backup removed");
    expect(steps[3]).toHaveTextContent("Run: Restart the gateway");
    expect(steps[4]).toHaveTextContent("Note: Other keys stay as they are");
  });

  it("shows the diff of a step, the token effect, the warnings and the undo command", async () => {
    render(<PlanPreview data={plan} />);
    expect(screen.getByLabelText("Before")).toHaveTextContent('"corp-tools": true');
    expect(screen.getByLabelText("After")).toHaveTextContent('"enabledPlugins": {}');
    expect(screen.getByText(/12,600 to 3,950 \(measured\)/)).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Claude Code has to be restarted in that folder",
    );
    expect(screen.getByText(plan.undo)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /copy/i }));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(plan.undo);
  });

  it("finds the plan in data.plan or in the data itself", () => {
    expect(planOf({ plan })).toBe(plan);
    expect(planOf(plan)).toBe(plan);
    expect(
      planOf({ plan: { summary: "x", steps: [{ op: "bogus", detail: "d" }] } }),
    ).toBeNull();
    expect(planOf(null)).toBeNull();
  });
});

describe("PlanPreview with the shapes of today's dry runs", () => {
  it("reads the profile edit dry run as a list, without the dryRun flag", () => {
    const { container } = render(
      <PlanPreview data={golden("profile-edit.preview.json")} />,
    );
    expect(screen.getByText("Old name")).toBeInTheDocument();
    expect(screen.getByText("Renamed")).toBeInTheDocument();
    expect(screen.getByText("Not in profile")).toBeInTheDocument();
    expect(screen.getByText("Added")).toBeInTheDocument();
    expect(screen.getAllByText("beta", { selector: "code" }).length).toBeGreaterThan(0);
    expect(screen.queryByText("Dry run")).not.toBeInTheDocument();
    expect(container.querySelector("pre")).toBeNull();
  });

  it("shows an empty list as none", () => {
    render(<PlanPreview data={golden("server-uninstall.preview.json")} />);
    expect(screen.getByText("Secrets removed")).toBeInTheDocument();
    expect(screen.getAllByText("none").length).toBeGreaterThanOrEqual(2);
  });

  it("falls back to pretty JSON for a shape it cannot list", () => {
    const data = golden("skills-sync.preview.json");
    const { container } = render(<PlanPreview data={data} />);
    const pre = container.querySelector("pre");
    expect(pre).not.toBeNull();
    expect(pre!.textContent).toContain('"entries": [');
    expect(pre!.textContent).toBe(JSON.stringify(data, null, 2));
  });

  it("renders a scalar, an array of objects and null without throwing", () => {
    const { container, rerender } = render(<PlanPreview data="just text" />);
    expect(container).toHaveTextContent("just text");
    rerender(<PlanPreview data={[{ a: 1 }]} />);
    expect(container.querySelector("pre")!.textContent).toBe(
      '[\n  {\n    "a": 1\n  }\n]',
    );
    rerender(<PlanPreview data={null} />);
    expect(container).toHaveTextContent("none");
  });
});

describe("plan helpers", () => {
  it("lists nested records up to three levels and JSONs anything deeper", () => {
    expect(toShown({ a: { b: { c: 1 } } }).kind).toBe("record");
    expect(toShown({ a: { b: { c: { d: 1 } } } }).kind).toBe("json");
    expect(toShown({ list: [1, "x", true] })).toEqual({
      kind: "record",
      entries: [["list", { kind: "list", items: ["1", "x", "yes"] }]],
    });
  });

  it("reads an applied ResultV1 and shows what changed and how to undo it", () => {
    const data = {
      result: {
        applied: true,
        changed: ["/work/app/CLAUDE.local.md"],
        undo: "toolportctl context bundle undo",
        backups: [],
      },
    };
    expect(resultOf(data)?.changed).toEqual(["/work/app/CLAUDE.local.md"]);
    expect(resultOf({ result: { applied: false } })).toBeNull();
    render(<PlanPreview data={data} />);
    expect(screen.getByText("/work/app/CLAUDE.local.md")).toBeInTheDocument();
    expect(screen.getByText("toolportctl context bundle undo")).toBeInTheDocument();
  });
});
