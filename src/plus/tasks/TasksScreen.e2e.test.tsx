import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import { nightlyReport, stockRuns } from "./fixtures";
import { calls, openTasks, pick, startRun } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";
import { CANARY } from "./world";
import type { TaskDefinition } from "../types/tasks";

let bridge: Bridge;
const quiet = () => stockRuns().filter((run) => run.task !== "portal-token");
const failing: TaskDefinition = {
  ...nightlyReport,
  id: "failing",
  title: "Failing job",
  steps: [{ id: "boom", title: "Boom", type: "exec", program: "false", args: [] }],
};

function setup(seed: Parameters<typeof createBridge>[0] = {}) {
  bridge = createBridge(seed);
  wire({ invoke, listen }, bridge);
}

beforeEach(() => setup());

describe("Tasks end to end against the task world", () => {
  it("opens from the sidebar view with its two tabs", async () => {
    render(<PlusViews view="tasks" onSelectView={() => {}} />);
    const tabs = await screen.findByRole("tablist", { name: "Tasks sections" });
    expect(within(tabs).getAllByRole("tab")).toHaveLength(2);
    expect(await screen.findByRole("list", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.queryByText("Not built yet")).toBeNull();
  });

  it("tasks.list reads task ls --all and nothing else to fill the screen", async () => {
    const user = await openTasks(bridge);
    await pick(user, /Nightly report/);
    await screen.findByRole("group", { name: "Nightly report" });
    expect(bridge.count("task ls --all")).toBe(1);
    expect(bridge.missing).toEqual([]);
  });

  it("tasks.detail reads task show for the selected task", async () => {
    const user = await openTasks(bridge);
    await pick(user, /Refresh the portal token/);
    await screen.findByRole("group", { name: "Refresh the portal token" });
    expect(bridge.count("task show portal-token")).toBe(1);
  });

  it("tasks.history reads the history and then one run with its log", async () => {
    const user = await openTasks(bridge, "History");
    const table = await screen.findByRole("table", { name: "Runs" });
    expect(bridge.count("task history --limit 50")).toBe(1);
    await user.click(
      within(table).getByRole("button", { name: "Log of run-fixture-ok" }),
    );
    await screen.findByLabelText("Log of Say hello");
    expect(bridge.count("task history --run run-fixture-ok")).toBeGreaterThan(0);
  });

  it("tasks.run previews first, starts only after the confirmation and waits at the step that needs you", async () => {
    setup({ runs: quiet() });
    const user = await openTasks(bridge);
    await pick(user, /Refresh the portal token/);
    const live = await startRun(user);
    expect(
      await live.findByText(/Sign in to the portal in the browser window/),
    ).toBeInTheDocument();
    expect(live.getByText("Waiting for you")).toBeInTheDocument();
    const order = calls(bridge, "task run");
    expect(order).toEqual(["task run portal-token --dry-run", "task run portal-token"]);
    expect(bridge.world.state.runs[0].status).toBe("waiting");
  });

  it("tasks.continue resumes the waiting run, which then finishes with a redacted log", async () => {
    setup({ runs: quiet() });
    const user = await openTasks(bridge);
    await pick(user, /Refresh the portal token/);
    const live = await startRun(user);
    await user.click(await live.findByRole("button", { name: "Continue" }));
    expect(await live.findByText("The run finished.")).toBeInTheDocument();
    expect(calls(bridge, "task resume")).toHaveLength(1);
    expect(live.getByLabelText("Log of Read the token")).toHaveTextContent(
      "([redacted])",
    );
    expect(live.getAllByText("Done").length).toBe(4);
    expect(document.body.innerHTML).not.toContain(CANARY);
    expect(JSON.stringify(bridge.calls)).not.toContain(CANARY);
  });

  it("tasks.run shows a run that fails with its step and error", async () => {
    setup({ tasks: [...createBridge().world.state.tasks, failing] });
    const user = await openTasks(bridge);
    await pick(user, /Failing job/);
    const live = await startRun(user);
    expect(await live.findByText("The run failed")).toBeInTheDocument();
    expect(live.getByText(/exit status 1/, { selector: "p" })).toBeInTheDocument();
    expect(calls(bridge, "task resume")).toHaveLength(0);
  });

  it("tasks.cancel stops a run that waits for you and nothing continues after it", async () => {
    setup({ runs: quiet() });
    const user = await openTasks(bridge);
    await pick(user, /Refresh the portal token/);
    const live = await startRun(user);
    await live.findByRole("button", { name: "Continue" });
    await user.click(live.getByRole("button", { name: /Cancel run/ }));
    expect(await live.findByText(/The run was cancelled/)).toBeInTheDocument();
    expect(calls(bridge, "task cancel")).toHaveLength(1);
    expect(bridge.world.state.written).toEqual([]);
  });

  it("tasks.add previews with the dry run, then saves the file it sent on stdin", async () => {
    const user = await openTasks(bridge);
    await user.click(await screen.findByRole("button", { name: /New task/ }));
    const form = within(await screen.findByRole("dialog", { name: "New task" }));
    await user.type(form.getByLabelText("Id"), "tidy-up");
    await user.type(form.getByLabelText("Title"), "Tidy up");
    await user.type(form.getByLabelText("Step id (step 1)"), "say");
    await user.type(form.getByLabelText("Step title (step 1)"), "Say");
    await user.type(form.getByLabelText("Instructions (step 1)"), "Tidy the desk");
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const box = within(await screen.findByRole("dialog", { name: "Add task tidy-up?" }));
    await user.click(await box.findByRole("button", { name: "Add task" }));
    await waitFor(() =>
      expect(bridge.world.state.tasks.map((t) => t.id)).toContain("tidy-up"),
    );
    expect(calls(bridge, "task add")).toEqual([
      "task add tidy-up --file - --dry-run",
      "task add tidy-up --file -",
    ]);
    const [dry, apply] = [
      bridge.stdin("task add tidy-up --file - --dry-run")[0],
      bridge.stdin("task add tidy-up --file -")[0],
    ];
    expect(dry).toBe(apply);
  });

  it("tasks.add --from-command previews and adds a disabled draft", async () => {
    const user = await openTasks(bridge);
    await user.click(
      await screen.findByRole("button", { name: /Create from a command/ }),
    );
    const form = within(
      await screen.findByRole("dialog", { name: "Create a task from a command" }),
    );
    await user.click(await form.findByRole("radio", { name: /refresh-login/ }));
    await user.click(form.getByRole("button", { name: "Review draft" }));
    const box = within(
      await screen.findByRole("dialog", {
        name: "Create task refresh-login from a command?",
      }),
    );
    await user.click(await box.findByRole("button", { name: "Create draft" }));
    await waitFor(() =>
      expect(
        bridge.world.state.tasks.find((t) => t.id === "refresh-login"),
      ).toMatchObject({
        enabled: false,
        createdFrom: { kind: "command" },
      }),
    );
    expect(calls(bridge, "task add")).toHaveLength(2);
  });

  it("tasks.edit sends the whole definition on stdin with the id unchanged", async () => {
    const user = await openTasks(bridge);
    await pick(user, /Nightly report/);
    await user.click(await screen.findByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit nightly-report" }),
    );
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Save task nightly-report?" }),
    );
    await box.findByText("Change task nightly-report");
    const sent = JSON.parse(
      bridge.stdin("task edit nightly-report --file - --dry-run")[0]!,
    );
    expect(sent).toEqual(nightlyReport);
    await user.click(box.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(calls(bridge, "task edit")).toHaveLength(2));
  });

  it("tasks.delete asks for the typed id, then removes the task", async () => {
    setup({ runs: [] });
    const user = await openTasks(bridge);
    await pick(user, /Clean up/);
    await user.click(await screen.findByRole("button", { name: "Delete…" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Delete task draft-cleanup?" }),
    );
    await box.findByText("Remove task draft-cleanup");
    await user.type(box.getByRole("textbox"), "draft-cleanup");
    await user.click(box.getByRole("button", { name: "Delete task" }));
    await waitFor(() =>
      expect(bridge.world.state.tasks.map((t) => t.id)).not.toContain("draft-cleanup"),
    );
    expect(calls(bridge, "task rm")).toEqual([
      "task rm draft-cleanup --dry-run",
      "task rm draft-cleanup",
    ]);
  });
});
