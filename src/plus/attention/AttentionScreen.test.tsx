import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { CtlReplyFailure } from "../fixtures/ctlReply";
import { AttentionScreen } from "./AttentionScreen";
import { dismissRow, openAttention, rowOf } from "./e2e";
import { makeItem } from "./fixtures";
import { untilDate } from "./model";
import { createBridge, wire, type Bridge } from "./testkit";

let bridge: Bridge;

function setup(seed: Parameters<typeof createBridge>[0] = {}) {
  bridge = createBridge(seed);
  wire({ invoke, listen }, bridge);
}

beforeEach(() => setup());

const writes = () =>
  bridge.ran().filter((argv) => !/^(commands|attention ls)/.test(argv));

describe("Attention screen groups and states", () => {
  it("shows the three groups in order with their counts, and a row says what, from where and how long", async () => {
    await openAttention(bridge);
    const groups = (await screen.findAllByRole("heading", { level: 3 })).map(
      (heading) => heading.textContent,
    );
    expect(groups).toEqual(["Needs you3", "Worth a look5", "For your information2"]);
    const row = await rowOf("alpha is missing a secret");
    expect(row.getByText(/Not set yet: API_KEY/)).toBeInTheDocument();
    expect(row.getByText("doctor")).toBeInTheDocument();
    expect(row.getByText("1 h")).toBeInTheDocument();
    const tiles = within(screen.getByText("Last check").parentElement!);
    expect(tiles.getByText(/\d/)).toBeInTheDocument();
  });

  it("counts the rows of every level in the tiles", async () => {
    await openAttention(bridge);
    for (const [label, value] of [
      ["Needs you", "3"],
      ["Worth a look", "5"],
      ["For your information", "2"],
    ]) {
      expect(
        screen.getByText(label, { selector: "span" }).parentElement,
      ).toHaveTextContent(value);
    }
  });

  it("says nothing needs you when the list is empty", async () => {
    setup({ items: [] });
    await openAttention(bridge);
    expect(await screen.findByText("Nothing needs you")).toBeInTheDocument();
    expect(screen.queryByRole("listitem")).toBeNull();
  });

  it("shows a loading state until the list answers", async () => {
    setup();
    let release!: () => void;
    bridge.set(
      "attention ls",
      () =>
        new Promise((resolve) => (release = () => resolve(bridge.world.list(undefined)))),
    );
    render(<AttentionScreen onNavigate={() => {}} />);
    expect(
      await screen.findByRole("status", { name: "Loading attention" }),
    ).toBeVisible();
    await act(async () => release());
    expect(await screen.findByRole("region", { name: "Needs you" })).toBeInTheDocument();
  });

  it("shows the CLI's own error with Retry when the list cannot be read", async () => {
    setup();
    bridge.set("attention ls", new CtlReplyFailure("internal", "the feed broke"));
    const user = userEvent.setup();
    render(<AttentionScreen onNavigate={() => {}} />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("the feed broke");
    bridge.set("attention ls", () => bridge.world.list(undefined));
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByRole("region", { name: "Needs you" })).toBeInTheDocument();
  });

  it("keeps the last answer on screen with a note when a refresh fails", async () => {
    const user = await openAttention(bridge);
    bridge.set("attention ls", new CtlReplyFailure("internal", "blip"));
    await user.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      /showing the last answer: blip/,
    );
    expect(screen.getByRole("region", { name: "Needs you" })).toBeInTheDocument();
  });

  it("tells the child process cannot run apart from a failed command", async () => {
    setup();
    bridge.down = true;
    const user = userEvent.setup();
    const onOpenCommands = vi.fn();
    render(<AttentionScreen onNavigate={() => {}} onOpenCommands={onOpenCommands} />);
    expect(await screen.findByText("Toolport can't run toolportctl")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Open doctor" }));
    expect(onOpenCommands).toHaveBeenCalledWith("doctor");
  });
});

describe("Attention screen dismissals", () => {
  it("previews with the dry run, hides nothing until confirmed and then runs the exact argv", async () => {
    const user = await openAttention(bridge);
    const week = untilDate("week")!;
    const title = "2 skills are installed but Claude cannot see them";
    await dismissRow(user, title, "For a week");
    const confirm = within(await screen.findByRole("dialog", { name: `Hide ${title}?` }));
    expect(confirm.getByText(`Hide skills:invisible until ${week}`)).toBeInTheDocument();
    expect(writes()).toEqual([
      `attention dismiss skills:invisible --until ${week} --dry-run`,
    ]);
    await user.click(confirm.getByRole("button", { name: "Hide" }));
    const done = within(await screen.findByRole("dialog", { name: `Hide ${title}` }));
    expect(
      await done.findByText(/attention dismiss skills:invisible --until/),
    ).toBeInTheDocument();
    expect(writes()).toEqual([
      `attention dismiss skills:invisible --until ${week} --dry-run`,
      `attention dismiss skills:invisible --until ${week}`,
    ]);
    await user.click(done.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() =>
      expect(screen.queryByRole("listitem", { name: title })).toBeNull(),
    );
  });

  it("hides for good with no --until", async () => {
    const user = await openAttention(bridge);
    await dismissRow(user, "alpha is missing a secret", "Forever");
    await user.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Hide",
      }),
    );
    await waitFor(() =>
      expect(writes()).toEqual([
        "attention dismiss secrets:srv-alpha:missing --dry-run",
        "attention dismiss secrets:srv-alpha:missing",
      ]),
    );
  });

  it("cancelling the plan applies nothing and keeps the row", async () => {
    const user = await openAttention(bridge);
    await dismissRow(user, "alpha is missing a secret");
    const confirm = await screen.findByRole("dialog", { name: /^Hide alpha/ });
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(writes()).toHaveLength(1);
    expect(
      screen.getByRole("listitem", { name: "alpha is missing a secret" }),
    ).toBeVisible();
  });

  it("closing the choice dialog runs nothing", async () => {
    const user = await openAttention(bridge);
    const row = await rowOf("alpha is missing a secret");
    await user.click(row.getByRole("button", { name: /^Dismiss/ }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(writes()).toEqual([]);
  });

  it("shows the failure of a dismissal and keeps the row", async () => {
    const user = await openAttention(bridge);
    bridge.set(
      `attention dismiss secrets:srv-alpha:missing --until ${untilDate("tomorrow")} --dry-run`,
      new CtlReplyFailure("io", "the attention file is read-only"),
    );
    await dismissRow(user, "alpha is missing a secret");
    const failed = within(await screen.findByRole("dialog"));
    expect(
      await failed.findByText(/the attention file is read-only/),
    ).toBeInTheDocument();
    await user.click(failed.getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(writes()).toHaveLength(1);
    expect(
      screen.getByRole("listitem", { name: "alpha is missing a secret" }),
    ).toBeVisible();
  });
});

describe("Attention screen links", () => {
  it("opens the screen a row names with its params", async () => {
    const onNavigate = vi.fn();
    const user = await openAttention(bridge, onNavigate);
    await user.click(
      (await rowOf("alpha is missing a secret")).getByRole("button", { name: /^Open/ }),
    );
    expect(onNavigate).toHaveBeenCalledWith("control", {
      server: "srv-alpha",
      tab: "secrets",
    });
    await user.click(
      (await rowOf("Task portal-token is waiting for you")).getByRole("button", {
        name: /^Open/,
      }),
    );
    expect(onNavigate).toHaveBeenLastCalledWith("tasks", {
      run: "run-fixture-waiting",
      tab: "tasks",
      task: "portal-token",
    });
  });

  it("offers no link for a route this build does not know", async () => {
    setup({
      items: [makeItem({ title: "Odd row", target: { route: "elsewhere", params: {} } })],
    });
    await openAttention(bridge);
    const row = await rowOf("Odd row");
    expect(row.queryByRole("button", { name: /^Open/ })).toBeNull();
    expect(row.getByRole("button", { name: /^Dismiss/ })).toBeEnabled();
  });
});
