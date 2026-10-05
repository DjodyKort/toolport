import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { CtlReplyFailure } from "../fixtures/ctlReply";
import type { TaskDefinition } from "../types/tasks";
import { nightlyReport, stockRuns } from "./fixtures";
import { TasksScreen } from "./TasksScreen";
import { createBridge, wire, type Bridge } from "./testkit";
import { CANARY } from "./world";

let bridge: Bridge;

const withoutWaitingRun = () => stockRuns().filter((run) => run.task !== "portal-token");
const failingTask: TaskDefinition = {
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

async function open(tab?: string) {
  const user = userEvent.setup();
  render(<TasksScreen pollMs={15} />);
  const tabs = await screen.findByRole("tablist", { name: "Tasks sections" });
  if (tab) await user.click(within(tabs).getByRole("tab", { name: tab }));
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  return user;
}

const list = () => screen.findByRole("list", { name: "Tasks" });
const detail = (title: string) => screen.findByRole("group", { name: title });

async function select(user: UserEvent, name: RegExp) {
  await user.click(within(await list()).getByRole("button", { name }));
}

async function confirmRun(user: UserEvent) {
  await user.click(await screen.findByRole("button", { name: "Run now…" }));
  const box = await screen.findByRole("dialog", { name: /^Run .*\?$/ });
  await user.click(await within(box).findByRole("button", { name: "Run now" }));
}

const pageText = () =>
  document.body.innerHTML +
  [...document.querySelectorAll("input,textarea")]
    .map((el) => (el as HTMLInputElement).value)
    .join("|");

describe("Tasks list", () => {
  it("has the Tasks and History tabs and opens on Tasks", async () => {
    await open();
    const tabs = screen.getByRole("tablist", { name: "Tasks sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["Tasks", "History"]);
    expect(within(tabs).getByRole("tab", { name: "Tasks" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("lists each task with its trigger chips, last run, next run and waiting state", async () => {
    await open();
    const rows = within(await list());
    const portal = rows.getByRole("button", { name: /Refresh the portal token/ });
    expect(portal).toHaveTextContent("Needs you");
    expect(portal).toHaveTextContent("CLI");
    expect(portal).toHaveTextContent("Claude may ask");
    expect(portal).toHaveTextContent("Login fails: srv-alpha");
    expect(portal).toHaveTextContent("Waiting for you");
    const nightly = rows.getByRole("button", { name: /Nightly report/ });
    expect(nightly).toHaveTextContent("Scheduled");
    expect(nightly).toHaveTextContent("Every day at 03:00");
    expect(nightly).toHaveTextContent("Last run ok");
    expect(nightly).toHaveTextContent("next");
    expect(rows.getByRole("button", { name: /Clean up/ })).toHaveTextContent("Off");
  });

  it("summarises the tasks and names a file that does not parse", async () => {
    await open();
    const strip = within(await screen.findByRole("group", { name: "Summary" }));
    expect(strip.getByText("Needs you")).toBeInTheDocument();
    expect(strip.getByText("portal-token")).toBeInTheDocument();
    expect(strip.getByText("nightly-report")).toBeInTheDocument();
    expect(await screen.findByText(/1 task file could not be read/)).toBeInTheDocument();
    expect(screen.getByText("broken")).toBeInTheDocument();
  });

  it("shows a loading state, then an empty state that offers both ways to start", async () => {
    let release!: (value: unknown) => void;
    const gate = new Promise((resolve) => (release = resolve));
    bridge.set("task ls --all", () => gate);
    render(<TasksScreen pollMs={15} />);
    expect(await screen.findByRole("status", { name: "Loading" })).toBeInTheDocument();
    release({ tasks: [], invalid: [] });
    expect(await screen.findByText("No tasks yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /New task/ })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Create from a command/ }),
    ).toBeInTheDocument();
  });

  it("shows what failed and retries", async () => {
    const user = userEvent.setup();
    let fail = true;
    bridge.set("task ls --all", () =>
      fail
        ? new CtlReplyFailure("bridge", "toolportctl could not start")
        : { tasks: [], invalid: [] },
    );
    render(<TasksScreen pollMs={15} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the tasks");
    expect(alert).toHaveTextContent("toolportctl could not start");
    fail = false;
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByText("No tasks yet")).toBeInTheDocument();
  });
});

describe("Task detail", () => {
  it("marks the steps that need you and names the secrets it may write, never a value", async () => {
    const user = await open();
    await select(user, /Refresh the portal token/);
    const card = within(await detail("Refresh the portal token"));
    const steps = within(await card.findByRole("region", { name: "What it does" }));
    expect(steps.getAllByText("Needs you")).toHaveLength(1);
    expect(steps.getAllByText("Auto")).toHaveLength(3);
    expect(
      steps.getByText(/Sign in to the portal in the browser window/),
    ).toBeInTheDocument();
    expect(card.getByText("API_KEY (srv-alpha)")).toBeInTheDocument();
    expect(
      card.getByText(/Write the secret srv-alpha\/API_KEY from a captured value/),
    ).toBeInTheDocument();
    const triggers = within(card.getByRole("list", { name: "Triggers" }));
    expect(triggers.getByText(/On a failed login of srv-alpha/)).toBeInTheDocument();
    expect(triggers.getByText("toolportctl task run portal-token")).toBeInTheDocument();
    expect(pageText()).not.toContain(CANARY);
  });

  it("lists the recent runs and opens the redacted log of one", async () => {
    const user = await open();
    await select(user, /Nightly report/);
    const card = within(await detail("Nightly report"));
    const runs = within(await card.findByRole("list", { name: "Recent runs" }));
    expect(runs.getByText("ok")).toBeInTheDocument();
    expect(runs.getByText("schedule")).toBeInTheDocument();
    await user.click(runs.getByRole("button", { name: /^Log of the run/ }));
    const log = within(
      await screen.findByRole("dialog", { name: "Log of run-fixture-ok" }),
    );
    expect(await log.findByLabelText("Log of Say hello")).toHaveTextContent("report");
    expect(log.getByText(/secret values never appear/)).toBeInTheDocument();
  });

  it("says a task was never run", async () => {
    setup({ runs: [] });
    const user = await open();
    await select(user, /Clean up/);
    expect(await screen.findByText("Never run in Toolport")).toBeInTheDocument();
  });

  it("shows why a detail could not be read", async () => {
    const user = await open();
    bridge.set("task show nightly-report", new CtlReplyFailure("not_found", "no task"));
    await select(user, /Nightly report/);
    expect(await screen.findByText("Couldn't read nightly-report")).toBeInTheDocument();
  });
});

describe("History", () => {
  it("lists every run with its result and trigger, and opens a log", async () => {
    const user = await open("History");
    const table = within(await screen.findByRole("table", { name: "Runs" }));
    expect(table.getAllByRole("row")).toHaveLength(4);
    expect(table.getByText("portal-token")).toBeInTheDocument();
    expect(table.getByText("Needs you")).toBeInTheDocument();
    await user.click(table.getByRole("button", { name: "Log of run-fixture-ok" }));
    const dialog = await screen.findByRole("dialog", { name: "Log of run-fixture-ok" });
    expect(await within(dialog).findByLabelText("Log of Say hello")).toHaveTextContent(
      "report",
    );
    expect(screen.getAllByText(/Logs are redacted/).length).toBeGreaterThan(1);
  });

  it("has an empty state and an error state", async () => {
    bridge.set("task history --limit 50", { runs: [] });
    const user = await open("History");
    expect(await screen.findByText("No runs yet")).toBeInTheDocument();
    bridge.set(
      "task history --limit 50",
      new CtlReplyFailure("failed", "history is unreadable"),
    );
    await user.click(screen.getByRole("tab", { name: "Tasks" }));
    await user.click(screen.getByRole("tab", { name: "History" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("history is unreadable");
  });
});

describe("Run", () => {
  it("previews the plan with the secrets, the servers and whether it waits, before anything runs", async () => {
    setup({ runs: withoutWaitingRun() });
    const user = await open();
    await select(user, /Refresh the portal token/);
    await user.click(await screen.findByRole("button", { name: "Run now…" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Run Refresh the portal token?" }),
    );
    expect(await box.findByText("Run task portal-token (4 steps)")).toBeInTheDocument();
    expect(box.getByText("API_KEY (srv-alpha)")).toBeInTheDocument();
    expect(box.getAllByText("srv-alpha").length).toBeGreaterThan(0);
    expect(box.getByText(/Yes: it pauses at a step that needs you/)).toBeInTheDocument();
    expect(bridge.count("task run portal-token --dry-run")).toBe(1);
    expect(bridge.count("task run portal-token")).toBe(0);
    await user.click(box.getByRole("button", { name: "Cancel" }));
    expect(bridge.count("task run portal-token")).toBe(0);
  });

  it("says a task runs on its own when it has no step that needs you", async () => {
    const user = await open();
    await select(user, /Nightly report/);
    await user.click(await screen.findByRole("button", { name: "Run now…" }));
    expect(await screen.findByText(/No: it runs on its own/)).toBeInTheDocument();
    expect(screen.getAllByText("no secrets").length).toBeGreaterThan(0);
  });

  it("runs, waits at the step that needs you, continues and finishes", async () => {
    setup({ runs: withoutWaitingRun() });
    const user = await open();
    await select(user, /Refresh the portal token/);
    await confirmRun(user);
    const live = within(
      await screen.findByRole("dialog", { name: "Run Refresh the portal token" }),
    );
    expect(
      await live.findByText(/Sign in to the portal in the browser window/),
    ).toBeInTheDocument();
    expect(live.getByText("Waiting for you")).toBeInTheDocument();
    await user.click(live.getByRole("button", { name: "Continue" }));
    expect(await live.findByText("The run finished.")).toBeInTheDocument();
    expect(
      bridge.ran().filter((argv) => argv.startsWith("task resume run-")),
    ).toHaveLength(1);
    expect(live.getByLabelText("Log of Read the token")).toHaveTextContent(
      "captured token ([redacted])",
    );
    expect(live.getByLabelText("Log of Store the token")).toHaveTextContent(
      "wrote srv-alpha/API_KEY from a captured value",
    );
    expect(bridge.world.state.written).toEqual(["srv-alpha/API_KEY"]);
    expect(pageText()).not.toContain(CANARY);
    expect(JSON.stringify(bridge.calls)).not.toContain(CANARY);
    await user.click(live.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows a failed run with the failing step and the redacted log", async () => {
    setup({ tasks: [...createBridge().world.state.tasks, failingTask] });
    const user = await open();
    await select(user, /Failing job/);
    await confirmRun(user);
    const live = within(await screen.findByRole("dialog", { name: "Run Failing job" }));
    expect(await live.findByText("The run failed")).toBeInTheDocument();
    expect(live.getByText(/step 1 \(Boom\) failed: exit status 1/)).toBeInTheDocument();
    expect(live.getByLabelText("Log of Boom")).toHaveTextContent("exit status 1");
  });

  it("cancels a run that waits for you", async () => {
    setup({ runs: withoutWaitingRun() });
    const user = await open();
    await select(user, /Refresh the portal token/);
    await confirmRun(user);
    const live = within(
      await screen.findByRole("dialog", { name: "Run Refresh the portal token" }),
    );
    await live.findByRole("button", { name: "Continue" });
    await user.click(live.getByRole("button", { name: /Cancel run/ }));
    expect(await live.findByText(/The run was cancelled/)).toBeInTheDocument();
    expect(
      bridge.ran().filter((argv) => argv.startsWith("task cancel run-")),
    ).toHaveLength(1);
    expect(live.queryByRole("button", { name: "Continue" })).toBeNull();
  });

  it("warns that a task is already running and shows the refusal when it is started", async () => {
    const user = await open();
    await select(user, /Refresh the portal token/);
    await user.click(await screen.findByRole("button", { name: "Run now…" }));
    const box = await screen.findByRole("dialog", {
      name: "Run Refresh the portal token?",
    });
    expect(
      await within(box).findByText(/already running as run-fixture-waiting/),
    ).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Run now" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("The run did not start");
    expect(alert).toHaveTextContent("conflict");
  });

  it("does not offer Run now for a task that is off", async () => {
    const user = await open();
    await select(user, /Clean up/);
    expect(await screen.findByRole("button", { name: "Run now…" })).toBeDisabled();
  });
});

describe("Add, edit and delete", () => {
  async function fillNew(user: UserEvent) {
    await user.click(await screen.findByRole("button", { name: /New task/ }));
    const form = within(await screen.findByRole("dialog", { name: "New task" }));
    await user.type(form.getByLabelText("Id"), "weekly-check");
    await user.type(form.getByLabelText("Title"), "Weekly check");
    await user.type(form.getByLabelText("Step id (step 1)"), "ask");
    await user.type(form.getByLabelText("Step title (step 1)"), "Ask");
    await user.type(form.getByLabelText("Instructions (step 1)"), "Open the report");
    return form;
  }

  it("checks the form before anything is previewed", async () => {
    const user = await open();
    await user.click(await screen.findByRole("button", { name: /New task/ }));
    const form = within(await screen.findByRole("dialog", { name: "New task" }));
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const alert = form.getByRole("alert");
    expect(alert).toHaveTextContent(/The id uses 1 to 64 characters/);
    expect(alert).toHaveTextContent("The task needs a title");
    expect(alert).toHaveTextContent("Step 1 needs an id");
    expect(bridge.ran().some((argv) => argv.startsWith("task add"))).toBe(false);
  });

  it("previews a new task, then adds it and shows it in the list", async () => {
    const user = await open();
    const form = await fillNew(user);
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Add task weekly-check?" }),
    );
    expect(await box.findByText("Add task weekly-check")).toBeInTheDocument();
    expect(box.getByText(/the task is disabled/)).toBeInTheDocument();
    const dry = "task add weekly-check --file /dev/stdin --dry-run";
    expect(bridge.count(dry)).toBe(1);
    const sent = JSON.parse(bridge.stdin(dry)[0]!);
    expect(sent).toMatchObject({
      id: "weekly-check",
      title: "Weekly check",
      enabled: false,
      steps: [{ id: "ask", type: "needs-you", instructions: "Open the report" }],
    });
    expect(bridge.world.state.tasks.map((t) => t.id)).not.toContain("weekly-check");
    await user.click(box.getByRole("button", { name: "Add task" }));
    await screen.findByText(/Applied|changed|task-runs|tasks\/weekly-check/i);
    expect(bridge.world.state.tasks.map((t) => t.id)).toContain("weekly-check");
    const done = await screen.findByRole("dialog", { name: "Add task weekly-check" });
    await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(
      await within(await list()).findByRole("button", { name: /Weekly check/ }),
    ).toBeInTheDocument();
  });

  it("keeps the form when the preview is refused", async () => {
    const user = await open();
    bridge.set(
      "task add weekly-check --file /dev/stdin --dry-run",
      new CtlReplyFailure("invalid_task", "the definition is not valid"),
    );
    const form = await fillNew(user);
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const failed = await screen.findByRole("dialog", { name: "Add task weekly-check" });
    expect(
      await within(failed).findByText(/the definition is not valid/),
    ).toBeInTheDocument();
    await user.click(within(failed).getAllByRole("button", { name: "Close" }).at(-1)!);
    const again = await screen.findByRole("dialog", { name: "New task" });
    expect(within(again).getByLabelText("Title")).toHaveValue("Weekly check");
  });

  it("edits a task: the form starts from its definition and the diff is previewed", async () => {
    const user = await open();
    await select(user, /Nightly report/);
    await user.click(await screen.findByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit nightly-report" }),
    );
    expect(form.getByLabelText("Id")).toBeDisabled();
    expect(form.getByLabelText("Program (step 1)")).toHaveValue("echo");
    const title = form.getByLabelText("Title");
    await user.clear(title);
    await user.type(title, "Nightly report v2");
    await user.click(form.getByRole("button", { name: "Review changes" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Save task nightly-report?" }),
    );
    expect(await box.findByText("Change task nightly-report")).toBeInTheDocument();
    await user.click(box.getByRole("button", { name: "Save task" }));
    await waitFor(() =>
      expect(bridge.world.state.tasks.find((t) => t.id === "nightly-report")?.title).toBe(
        "Nightly report v2",
      ),
    );
    const done = await screen.findByRole("dialog", { name: "Save task nightly-report" });
    await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(
      await within(await list()).findByRole("button", { name: /Nightly report v2/ }),
    ).toBeInTheDocument();
  });

  it("duplicates a task into a new draft", async () => {
    const user = await open();
    await select(user, /Nightly report/);
    await user.click(await screen.findByRole("button", { name: "Duplicate" }));
    const form = within(await screen.findByRole("dialog", { name: "New task" }));
    expect(form.getByLabelText("Id")).toHaveValue("");
    expect(form.getByLabelText("Title")).toHaveValue("Nightly report copy");
    expect(form.getByRole("checkbox", { name: /Enabled/ })).not.toBeChecked();
  });

  it("deletes a task only after its id is typed", async () => {
    setup({ runs: [] });
    const user = await open();
    await select(user, /Clean up/);
    await user.click(await screen.findByRole("button", { name: "Delete…" }));
    const box = within(
      await screen.findByRole("dialog", { name: "Delete task draft-cleanup?" }),
    );
    await box.findByText("Remove task draft-cleanup");
    const apply = box.getByRole("button", { name: "Delete task" });
    expect(apply).toBeDisabled();
    await user.type(box.getByRole("textbox"), "draft-cleanup");
    await user.click(apply);
    await waitFor(() =>
      expect(bridge.world.state.tasks.map((t) => t.id)).not.toContain("draft-cleanup"),
    );
    const done = await screen.findByRole("dialog", { name: "Delete task draft-cleanup" });
    await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Tasks" })).queryByText("Clean up"),
      ).toBeNull(),
    );
  });

  it("refuses to delete a task that is running", async () => {
    const user = await open();
    await select(user, /Refresh the portal token/);
    await user.click(await screen.findByRole("button", { name: "Delete…" }));
    const failed = await screen.findByRole("dialog", {
      name: "Delete task portal-token",
    });
    expect(await within(failed).findByText(/cancel the run first/)).toBeInTheDocument();
    expect(bridge.count("task rm portal-token")).toBe(0);
  });
});

describe("Create from a command", () => {
  it("lists the command files, marks the ones that look like tasks and adds a draft", async () => {
    const user = await open();
    await user.click(
      await screen.findByRole("button", { name: /Create from a command/ }),
    );
    const form = within(
      await screen.findByRole("dialog", { name: "Create a task from a command" }),
    );
    const radios = await form.findByRole("radiogroup", { name: "Commands" });
    expect(within(radios).getByText("Looks like a task")).toBeInTheDocument();
    expect(within(radios).getAllByRole("radio")).toHaveLength(2);
    expect(form.getByRole("button", { name: "Review draft" })).toBeDisabled();
    await user.click(within(radios).getByRole("radio", { name: /refresh-login/ }));
    expect(form.getByLabelText("Task id")).toHaveValue("refresh-login");
    await user.click(form.getByRole("button", { name: "Review draft" }));
    const box = within(
      await screen.findByRole("dialog", {
        name: "Create task refresh-login from a command?",
      }),
    );
    expect(await box.findByText("Add task refresh-login")).toBeInTheDocument();
    const dry =
      "task add refresh-login --from-command /home/demo/.claude/commands/refresh-login.md --dry-run";
    expect(bridge.count(dry)).toBe(1);
    await user.click(box.getByRole("button", { name: "Create draft" }));
    await waitFor(() =>
      expect(
        bridge.world.state.tasks.find((t) => t.id === "refresh-login")?.enabled,
      ).toBe(false),
    );
  });

  it("says no command was found and takes a path instead", async () => {
    setup({ commandFiles: [] });
    const user = await open();
    await user.click(
      await screen.findByRole("button", { name: /Create from a command/ }),
    );
    const form = within(
      await screen.findByRole("dialog", { name: "Create a task from a command" }),
    );
    expect(await form.findByText(/No command files were found/)).toBeInTheDocument();
    await user.type(
      form.getByLabelText("Command file"),
      "/home/demo/notes/Renew Login.md",
    );
    expect(form.getByLabelText("Task id")).toHaveValue("renew-login");
    expect(form.getByRole("button", { name: "Review draft" })).toBeEnabled();
  });

  it("does not take an id that exists", async () => {
    const user = await open();
    await user.click(
      await screen.findByRole("button", { name: /Create from a command/ }),
    );
    const form = within(
      await screen.findByRole("dialog", { name: "Create a task from a command" }),
    );
    await user.type(form.getByLabelText("Command file"), "/x/portal-token.md");
    expect(form.getByText("A task with this id exists")).toBeInTheDocument();
    expect(form.getByRole("button", { name: "Review draft" })).toBeDisabled();
  });
});

describe("Keyboard and states", () => {
  it("moves between the tabs with the arrow keys", async () => {
    await open();
    const tabs = screen.getByRole("tablist", { name: "Tasks sections" });
    const user = userEvent.setup();
    within(tabs).getByRole("tab", { name: "Tasks" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(within(tabs).getByRole("tab", { name: "History" })).toHaveFocus();
    expect(await screen.findByRole("table", { name: "Runs" })).toBeInTheDocument();
  });

  it("reaches every task row with the keyboard and selects one with Enter", async () => {
    const user = await open();
    const rows = within(await list()).getAllByRole("button");
    rows[1].focus();
    await user.keyboard("{Enter}");
    expect(rows[1]).toHaveAttribute("aria-current", "true");
    expect(await detail("Nightly report")).toBeInTheDocument();
  });

  it("closes the run preview with Escape and starts nothing", async () => {
    const user = await open();
    await select(user, /Nightly report/);
    await user.click(await screen.findByRole("button", { name: "Run now…" }));
    await screen.findByRole("dialog", { name: "Run Nightly report?" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count("task run nightly-report")).toBe(0);
  });
});
