import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

const shown = (name: string) =>
  function Stub(props: Record<string, unknown>) {
    return (
      <p>
        {name} opened on tab {String(props.initialTab)} task {String(props.initialTask)}
      </p>
    );
  };
vi.mock("../servers/ServersScreen", () => ({ ServersScreen: shown("Servers") }));
vi.mock("../tasks/TasksScreen", () => ({ TasksScreen: shown("Tasks") }));
vi.mock("../skills/LibraryScreen", () => ({ LibraryScreen: shown("Library") }));
vi.mock("../context/ContextScreen", () => ({ ContextScreen: shown("Context") }));
vi.mock("../compression/TokensScreen", () => ({ TokensScreen: shown("Tokens") }));
vi.mock("../system/SystemScreen", () => ({ SystemScreen: shown("System") }));

import type { View } from "@/lib/types";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { forgetAttentionProbe } from "../attention";
import { isPlusView } from "../nav";
import { PlusViews } from "../PlusViews";
import { NavRow, SidebarNav } from "../SidebarNav";
import { makeItem } from "./fixtures";
import { dismissRow, rowOf } from "./e2e";
import { untilDate } from "./model";
import { createBridge, wire, type Bridge } from "./testkit";
import { CANARY, leakyItem } from "./world";

let bridge: Bridge;

function setup(seed: Parameters<typeof createBridge>[0] = {}) {
  bridge = createBridge(seed);
  wire({ invoke, listen }, bridge);
}

beforeEach(() => {
  forgetAttentionProbe();
  setup();
});

const row: NavRow = (Icon, label, active, onClick, badge, _stale, badgeLabel) => (
  <button type="button" aria-current={active ? "page" : undefined} onClick={onClick}>
    <Icon aria-hidden="true" />
    {label}
    {badge !== undefined && badge !== null && badge > 0 && (
      <span aria-label={badgeLabel}>{badge}</span>
    )}
  </button>
);

function Harness() {
  const [view, setView] = useState<View>("attention");
  return (
    <>
      <nav aria-label="Sidebar">
        <SidebarNav
          view={view}
          onSelectView={setView}
          row={row}
          quarantined={null}
          quarantineStale={false}
        />
      </nav>
      <output aria-label="view">{view}</output>
      {isPlusView(view) ? (
        <PlusViews view={view} onSelectView={setView} />
      ) : (
        <p>upstream</p>
      )}
    </>
  );
}

async function openAttention() {
  const user = userEvent.setup();
  render(<Harness />);
  await screen.findByRole("region", { name: "Needs you" });
  await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
  return user;
}

const writes = () =>
  bridge.ran().filter((argv) => !/^(commands|attention ls)/.test(argv));

describe("Attention end to end against the attention world", () => {
  it("attention.ls reads the list once, the registry for the policies, and runs nothing that writes", async () => {
    await openAttention();
    expect(bridge.count("attention ls")).toBe(1);
    expect(bridge.count("commands")).toBeGreaterThan(0);
    expect(writes()).toEqual([]);
    expect(bridge.missing).toEqual([]);
    expect(screen.queryByText("Not built yet")).toBeNull();
    expect(
      within(screen.getByRole("region", { name: "Worth a look" })).getAllByRole(
        "listitem",
      ),
    ).toHaveLength(5);
  });

  it("attention.dismiss previews with --dry-run, applies the same argv without it and the next list leaves the row out", async () => {
    const user = await openAttention();
    const until = untilDate("tomorrow");
    await dismissRow(user, "alpha is missing a secret");
    const confirm = within(await screen.findByRole("dialog", { name: /^Hide alpha/ }));
    await user.click(confirm.getByRole("button", { name: "Hide" }));
    const done = within(await screen.findByRole("dialog", { name: /^Hide alpha/ }));
    expect(
      await done.findByText(/attention dismiss secrets:srv-alpha:missing/),
    ).toBeVisible();
    await user.click(done.getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(writes()).toEqual([
      `attention dismiss secrets:srv-alpha:missing --until ${until} --dry-run`,
      `attention dismiss secrets:srv-alpha:missing --until ${until}`,
    ]);
    await waitFor(() =>
      expect(
        screen.queryByRole("listitem", { name: "alpha is missing a secret" }),
      ).toBeNull(),
    );
    expect(bridge.world.state.hidden["secrets:srv-alpha:missing"]).toBe(until);
  });

  it.each([
    [
      "alpha is missing a secret",
      "Servers opened on tab secrets task undefined",
      "control",
    ],
    [
      "Task portal-token is waiting for you",
      "Tasks opened on tab tasks task portal-token",
      "tasks",
    ],
    [
      "2 skills are installed but Claude cannot see them",
      "Library opened on tab skills task undefined",
      "library",
    ],
    [
      "Bundle acme-dev changed since it was applied",
      "Context opened on tab profiles task undefined",
      "context",
    ],
    [
      "Compression presets differ from the installed engine",
      "Tokens opened on tab compression task undefined",
      "tokens",
    ],
    [
      "Your skills library is behind its remote",
      "System opened on tab sync task undefined",
      "system",
    ],
  ])(
    "the link of %s opens the screen that fixes it on the right tab",
    async (title, text, view) => {
      const user = await openAttention();
      await user.click((await rowOf(title)).getByRole("button", { name: /^Open/ }));
      expect(await screen.findByText(text)).toBeInTheDocument();
      expect(screen.getByLabelText("view")).toHaveTextContent(view);
    },
  );

  it("opens a screen on its first tab again after the person left it through the sidebar", async () => {
    const user = await openAttention();
    await user.click(
      (await rowOf("alpha is missing a secret")).getByRole("button", { name: /^Open/ }),
    );
    await screen.findByText("Servers opened on tab secrets task undefined");
    await user.click(screen.getByRole("button", { name: /^Attention/ }));
    await screen.findByRole("region", { name: "Needs you" });
    await user.click(screen.getByRole("button", { name: /^Servers/ }));
    expect(
      await screen.findByText("Servers opened on tab undefined task undefined"),
    ).toBeVisible();
  });

  it("runs an action with exactly its argv and its dry-run twin, and nothing else", async () => {
    const user = await openAttention();
    const title = "Bundle acme-dev changed since it was applied";
    await user.click(
      (await rowOf(title)).getByRole("button", { name: `Apply again: ${title}` }),
    );
    const confirm = within(
      await screen.findByRole("dialog", { name: `Apply again: ${title}?` }),
    );
    const exact = "context bundle apply acme-dev --cwd /home/demo/work/acme-erp";
    expect(
      confirm.getAllByText(/toolportctl context bundle apply acme-dev/),
    ).not.toHaveLength(0);
    await user.click(confirm.getByRole("button", { name: "Apply again" }));
    await waitFor(() => expect(writes()).toEqual([`${exact} --dry-run`, exact]));
    expect(bridge.world.state.ran).toEqual([exact]);
  });

  it("refuses an action whose command the registry does not know, and the bridge never sees it", async () => {
    setup({
      items: [
        makeItem({
          level: "needs-you",
          title: "Odd row",
          action: { label: "Fix it", command: ["toolportctl", "nuke", "--all"] },
        }),
      ],
    });
    const user = await openAttention();
    await user.click(
      (await rowOf("Odd row")).getByRole("button", { name: "Fix it: Odd row" }),
    );
    expect(await screen.findByText(/does not know how safe/)).toBeVisible();
    expect(writes()).toEqual([]);
  });

  it("keeps the sidebar counter equal to counts.needsYou, and refreshes it after a dismissal", async () => {
    const user = await openAttention();
    const needsYou = () => bridge.world.list(undefined).counts.needsYou;
    expect(needsYou()).toBe(3);
    expect(await screen.findByLabelText("3 need you")).toHaveTextContent("3");
    expect(bridge.count("attention ls --level needs-you")).toBeGreaterThan(0);
    await dismissRow(user, "alpha is missing a secret", "Forever");
    await user.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Hide",
      }),
    );
    const done = within(await screen.findByRole("dialog"));
    await done.findByText(/attention dismiss/);
    await user.click(done.getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(needsYou()).toBe(2);
    expect(await screen.findByLabelText("2 need you")).toHaveTextContent("2");
    expect(
      screen.getByText("Needs you", { selector: "span" }).parentElement,
    ).toHaveTextContent("2");
  });

  it("hides the counter when nothing needs you", async () => {
    setup({ items: [makeItem({ level: "look" })] });
    forgetAttentionProbe();
    render(<Harness />);
    await screen.findByRole("region", { name: "Worth a look" });
    await waitFor(() => expect(bridge.count("attention ls --level needs-you")).toBe(1));
    expect(screen.queryByLabelText(/need you$/)).toBeNull();
  });

  it("can be used with the keyboard: Tab reaches Dismiss, Enter opens the choice, Escape closes it and focus comes back", async () => {
    const user = await openAttention();
    await screen.findByRole("button", { name: "Check again" });
    const dismiss = (await rowOf("alpha is missing a secret")).getByRole("button", {
      name: /^Dismiss/,
    });
    dismiss.focus();
    await user.keyboard("{Enter}");
    const box = await screen.findByRole("dialog", { name: "Hide this row?" });
    await user.keyboard("{ArrowDown}");
    expect(within(box).getByRole("radio", { name: "For a week" })).toBeChecked();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
    await waitFor(() => expect(dismiss).toHaveFocus());
  });

  it("shows that toolportctl cannot run, and the list after Retry once it can", async () => {
    setup();
    bridge.down = true;
    forgetAttentionProbe();
    const user = userEvent.setup();
    render(<Harness />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Toolport can't run toolportctl");
    bridge.down = false;
    await user.click(within(alert).getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("region", { name: "Needs you" })).toBeVisible();
  });

  it("shows a failed command with its own message instead of an empty screen", async () => {
    setup();
    bridge.set("attention ls", new CtlReplyFailure("internal", "the feed broke"));
    render(<Harness />);
    expect(await screen.findByRole("alert")).toHaveTextContent("the feed broke");
  });

  it("never sends what a row's detail carries to the bridge, and shows no secret-shaped value for the stock rows", async () => {
    await openAttention();
    expect(document.body.textContent).not.toContain(CANARY);
    const rows = screen.getAllByRole("listitem").map((item) => item.textContent);
    expect(rows.join(" ")).not.toMatch(/[A-Za-z0-9_-]{32,}/);
    expect(JSON.stringify(bridge.world.state.items)).not.toContain(CANARY);
  });

  it("keeps a canary of a row's detail out of every argv of a dismissal and an action", async () => {
    const other = {
      ...leakyItem,
      id: "auth:srv-leaky2",
      title: "leaky2 needs a new sign-in",
    };
    setup({ items: [leakyItem, { ...other, action: null }] });
    const user = userEvent.setup();
    render(<Harness />);
    await screen.findByRole("listitem", { name: leakyItem.title });
    await waitFor(() => expect(bridge.count("commands")).toBeGreaterThan(0));
    await user.click(
      (await rowOf(leakyItem.title)).getByRole("button", {
        name: `Check again: ${leakyItem.title}`,
      }),
    );
    await screen.findByRole("dialog", { name: /^Check again/ });
    await user.click(screen.getAllByRole("button", { name: "Close" }).at(-1)!);
    await dismissRow(user, other.title, "Forever");
    await user.click(
      await within(await screen.findByRole("dialog")).findByRole("button", {
        name: "Hide",
      }),
    );
    await waitFor(() => expect(writes()).toHaveLength(3));
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    expect(JSON.stringify(bridge.calls)).not.toContain(CANARY);
  });
});
