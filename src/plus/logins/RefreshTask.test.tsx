import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { goldenData } from "../tasks/fixtures";
import { renderTab, wire } from "./harness";
import { createBridge, ctlReplyFailure, type Bridge } from "./testkit";

let bridge: Bridge;

type Run = { id: string; task: string; steps: unknown[]; [key: string]: unknown };
const preview = { ...goldenData<object>("task-run.preview"), task: "erp-token" };
const started = goldenData<{ run: Run }>("task-run.apply");
const finished = {
  run: {
    ...goldenData<{ run: Run }>("task-history.run").run,
    id: "run-000",
    task: "erp-token",
  },
};

beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
  bridge.set("task run erp-token --dry-run", preview);
  bridge.set("task run erp-token", { ...started, task: "erp-token" });
  bridge.set("task history --run run-000", finished);
});

async function openLogins() {
  const view = renderTab("logins");
  const table = await screen.findByRole("table", { name: "Logins" });
  return { ...view, table };
}

describe("Refresh task in Logins", () => {
  it("shows the action on a server that has a refresh task, signed in or not", async () => {
    const { table } = await openLogins();
    expect(
      await within(table).findByRole("button", { name: "Refresh task for acme-erp" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("button", { name: "Refresh task for issue-tracker" }),
    ).toBeInTheDocument();
    expect(
      within(table).queryByRole("button", { name: "Refresh task for corp-tools" }),
    ).toBeNull();
  });

  it("opens the run dialog of that task: plan, confirm, then the finished run", async () => {
    const { user, table } = await openLogins();
    await user.click(
      await within(table).findByRole("button", { name: "Refresh task for acme-erp" }),
    );
    const box = within(
      await screen.findByRole("dialog", { name: "Run Renew the srv-erp login?" }),
    );
    expect(await box.findByText(/Run task portal-token/)).toBeInTheDocument();
    expect(bridge.count("task run erp-token --dry-run")).toBe(1);
    expect(bridge.count("task run erp-token")).toBe(0);
    await user.click(box.getByRole("button", { name: "Run now" }));
    const live = within(
      await screen.findByRole("dialog", { name: "Run Renew the srv-erp login" }),
    );
    expect(await live.findByText("The run finished.")).toBeInTheDocument();
    expect(bridge.count("task run erp-token")).toBe(1);
    await user.click(live.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("offers nothing when the tasks cannot be read", async () => {
    bridge.set("task ls", ctlReplyFailure("bridge", "tasks are unreadable"));
    const { table } = await openLogins();
    await within(table).findByRole("button", { name: "Probe acme-erp" });
    expect(within(table).queryByRole("button", { name: /Refresh task/ })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not offer a task that is off", async () => {
    const off = {
      tasks: [
        { ...goldenData<{ tasks: object[] }>("task-ls.all").tasks[0], enabled: false },
      ],
      invalid: [],
    };
    bridge.set("task ls", off);
    const { table } = await openLogins();
    await within(table).findByRole("button", { name: "Probe acme-erp" });
    expect(within(table).queryByRole("button", { name: /Refresh task/ })).toBeNull();
  });
});
