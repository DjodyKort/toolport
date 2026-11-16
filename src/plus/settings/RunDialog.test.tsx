import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RunDialog, type RunTarget } from "./RunDialog";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

function answer(data: unknown, ok = true) {
  invoke.mockImplementation(async (command: string) => {
    if (command === "plus_ctl") return "job-1";
    return {
      job: "job-1",
      exitCode: ok ? 0 : 1,
      signal: null,
      cancelled: false,
      envelope: { ok, command: "mcp", schemaVersion: 1, data },
      parseError: null,
      stderr: [],
      truncated: false,
    };
  });
}

const gated: RunTarget = {
  label: "toolportctl mcp call servers_remove",
  argv: ["mcp", "call", "servers_remove", "--args-stdin"],
  tool: { name: "servers_remove", gated: true, previews: true, hasParams: true },
};

beforeEach(() => invoke.mockReset());

describe("RunDialog", () => {
  it("test01_a_gated_tool_needs_its_name_typed_before_it_runs_with_confirm", async () => {
    answer({ removed: true });
    render(<RunDialog target={gated} onClose={vi.fn()} />);
    const run = screen.getByRole("button", { name: "Run it" });
    expect(run).toBeDisabled();
    await userEvent.type(
      screen.getByLabelText("Type servers_remove to confirm"),
      "servers_remove",
    );
    expect(run).toBeEnabled();
    await userEvent.click(run);
    await waitFor(() => expect(screen.getByText("Done")).toBeInTheDocument());
    const start = invoke.mock.calls.find(([command]) => command === "plus_ctl");
    expect(start?.[1]).toEqual({
      argv: gated.argv,
      stdinSecret: JSON.stringify({ confirm: true, dry_run: false }),
    });
  });

  it("test02_preview_runs_the_tool_with_dry_run_and_no_confirm", async () => {
    answer({ plan: [] });
    render(<RunDialog target={gated} onClose={vi.fn()} />);
    await userEvent.click(screen.getByRole("button", { name: "Preview" }));
    await waitFor(() => expect(screen.getByText("Done")).toBeInTheDocument());
    const start = invoke.mock.calls.find(([command]) => command === "plus_ctl");
    expect(JSON.parse(start?.[1].stdinSecret)).toEqual({ dry_run: true });
  });

  it("test03_a_failed_envelope_shows_the_error_not_a_success", async () => {
    invoke.mockImplementation(async (command: string) =>
      command === "plus_ctl"
        ? "job-1"
        : {
            job: "job-1",
            exitCode: 2,
            signal: null,
            cancelled: false,
            envelope: {
              ok: false,
              command: "mcp",
              schemaVersion: 1,
              error: { code: "usage", message: "unknown tool: nope" },
            },
            parseError: null,
            stderr: [],
            truncated: false,
          },
    );
    render(
      <RunDialog
        target={{ label: "toolportctl status", argv: ["status"] }}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText("Failed")).toBeInTheDocument();
    expect(screen.getByText("unknown tool: nope")).toBeInTheDocument();
  });

  it("test04_a_rejected_start_is_reported", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "plus_ctl") throw "toolportctl not found: next to the app";
      return null;
    });
    render(
      <RunDialog
        target={{ label: "toolportctl status", argv: ["status"] }}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByText("Failed")).toBeInTheDocument();
    expect(screen.getByLabelText("Output")).toHaveTextContent("toolportctl not found");
  });
});
