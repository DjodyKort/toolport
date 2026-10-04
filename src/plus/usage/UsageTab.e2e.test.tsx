import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { PlusViews } from "../PlusViews";
import { exact, tokensOf, type Counts } from "./model";
import {
  bridgeDown,
  createBridge,
  failure,
  goldenData,
  wire,
  type Bridge,
} from "./testkit";
import {
  OTEL_ONLY_MESSAGE,
  aggregate,
  createOtelWorld,
  emptyUsage,
  usageWorld,
  worldMessages,
  type Msg,
  type OtelWorld,
} from "./world";

/** The Usage tab walked the way a person uses it, through the Tokens screen: the host opens on
 * the tab, a Refresh changes the figures, an Enable or Disable changes the receiver the next
 * status reports. Each test is named by the parity action it proves (`gui-parity.json`). */
const NOW = "2026-10-04T12:00:00Z";
const PORT = 4318;

const NEW_MESSAGE: Msg = {
  id: "s-erp-2-new-1",
  day: "2026-10-04",
  ts: "2026-10-04T09:30:00Z",
  session: "s-erp-2",
  cwd: "/work/acme-erp",
  model: "claude-b",
  input: 1_000,
  output: 2_000,
  cacheCreation: 0,
  cacheRead: 0,
  tools: [],
};

const refreshedWorld = () => ({
  ...usageWorld(),
  ...aggregate([...worldMessages(), OTEL_ONLY_MESSAGE, NEW_MESSAGE]),
  index: { files: 5, messages: worldMessages().length + 1 },
});

const sumOf = (messages: Msg[]): Counts => ({
  messages: messages.length,
  input: messages.reduce((n, m) => n + m.input, 0),
  output: messages.reduce((n, m) => n + m.output, 0),
  cacheCreation: messages.reduce((n, m) => n + m.cacheCreation, 0),
  cacheRead: messages.reduce((n, m) => n + m.cacheRead, 0),
});

const since = (day: string) =>
  [...worldMessages(), OTEL_ONLY_MESSAGE].filter((msg) => msg.day >= day);

let bridge: Bridge;
let otel: OtelWorld;

function start(options: { receiver?: boolean } = {}) {
  otel = createOtelWorld({ enabled: options.receiver ?? false });
  bridge = createBridge();
  bridge.set("obs otel status", () => otel.status());
  bridge.set(`obs otel enable --port ${PORT} --dry-run`, () => otel.enable(PORT, true));
  bridge.set(`obs otel enable --port ${PORT}`, () => otel.enable(PORT, false));
  bridge.set("obs otel disable --dry-run", () => otel.disable(true));
  bridge.set("obs otel disable", () => otel.disable(false));
  wire({ invoke, listen }, bridge);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  start();
});

afterEach(() => {
  vi.useRealTimers();
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter(
      (line) => !/^(commands|usage|obs otel (status|enable|disable))( |$)/.test(line),
    );
  expect(stray, "the tab only runs its own commands").toEqual([]);
  expect(
    bridge.ran().some((line) => /--home|secret|--reveal|stdin|token/.test(line)),
  ).toBe(false);
});

async function openUsage() {
  const user = userEvent.setup();
  render(<PlusViews view="tokens" onSelectView={() => {}} />);
  const tabs = await screen.findByRole("tablist", { name: "Tokens sections" });
  expect(within(tabs).getByRole("tab", { name: "Usage" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  return user;
}

const strip = () => screen.findByRole("group", { name: "Usage summary" });
const card = () =>
  screen.getByRole("region", { name: "OpenTelemetry receiver", hidden: true });
const button = (name: string | RegExp) => screen.getByRole("button", { name });

async function closeResult(user: UserEvent) {
  const done = await screen.findByRole("dialog", { name: (name) => !name.endsWith("?") });
  await waitFor(() =>
    expect(within(done).getAllByRole("button", { name: "Close" }).length).toBeGreaterThan(
      1,
    ),
  );
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

describe("Usage tab, end to end: usage", () => {
  it("usage.refresh: opens on the stored index through the Tokens screen and works out the same numbers", async () => {
    await openUsage();
    const group = await strip();
    const window = sumOf(since("2026-09-21"));
    expect(group.querySelector(`[title="${exact(tokensOf(window))}"]`)).not.toBeNull();
    expect(within(group).getByText(`${exact(window.messages)} messages`)).toBeVisible();
    expect(screen.getByRole("region", { name: "By project" })).toBeVisible();
    expect(screen.getByRole("img", { name: /Tokens per day/ })).toBeVisible();
    expect(bridge.count("usage --no-refresh")).toBe(1);
    expect(bridge.count("usage")).toBe(0);
  });

  it("usage.refresh: Refresh re-indexes and the figures and the time of the refresh change", async () => {
    const user = await openUsage();
    const before = sumOf(since("2026-09-21"));
    await strip();
    bridge.set("usage", refreshedWorld());
    await user.click(button("Refresh"));
    const after = tokensOf(before) + 3_000;
    await waitFor(() =>
      expect(
        screen
          .getByRole("group", { name: "Usage summary" })
          .querySelector(`[title="${exact(after)}"]`),
      ).not.toBeNull(),
    );
    expect(
      within(screen.getByRole("group", { name: "Usage summary" })).getByText(
        `${exact(before.messages + 1)} messages`,
      ),
    ).toBeVisible();
    expect(
      within(
        screen.getByRole("region", { name: "Where these numbers come from" }),
      ).getByText("2026-10-04 12:00 UTC, by Refresh in this window"),
    ).toBeVisible();
    expect(bridge.count("usage")).toBe(1);
    expect(bridge.count("usage --no-refresh")).toBe(1);
  });

  it("usage.refresh: an index that never ran says so and Refresh builds it", async () => {
    bridge.set("usage --no-refresh", emptyUsage());
    bridge.set("usage", usageWorld());
    const user = await openUsage();
    expect(await screen.findByText("Nothing indexed yet")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Usage summary" })).toBeNull();
    expect(within(card()).getByRole("button", { name: "Enable…" })).toBeEnabled();
    await user.click(screen.getAllByRole("button", { name: "Refresh" }).at(-1)!);
    await strip();
    expect(screen.queryByText("Nothing indexed yet")).toBeNull();
    expect(screen.getByRole("region", { name: "By MCP server" })).toBeVisible();
    expect(bridge.count("usage")).toBe(1);
  });

  it("usage.refresh: a folder is passed to both reads as --root", async () => {
    bridge.set("usage --no-refresh --root /fixture/projects", usageWorld());
    bridge.set("usage --root /fixture/projects", refreshedWorld());
    const user = await openUsage();
    await strip();
    await user.type(screen.getByLabelText(/Transcript folder/), "/fixture/projects");
    await user.click(button("Use this folder"));
    await waitFor(() =>
      expect(bridge.ran()).toContain("usage --no-refresh --root /fixture/projects"),
    );
    await user.click(button("Refresh"));
    await waitFor(() => expect(bridge.ran()).toContain("usage --root /fixture/projects"));
  });

  it("usage.refresh: the bridge down says toolportctl could not run, and Retry reads again", async () => {
    bridge.set("usage --no-refresh", bridgeDown("toolportctl is not installed"));
    const user = await openUsage();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Toolport could not run toolportctl");
    expect(screen.queryByRole("group", { name: "Usage summary" })).toBeNull();
    bridge.set("usage --no-refresh", usageWorld());
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    await strip();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(bridge.count("usage --no-refresh")).toBe(2);
  });

  it("usage.refresh: a failed re-index keeps the stored figures and offers Retry", async () => {
    const user = await openUsage();
    await strip();
    bridge.set("usage", failure("usage", "cannot read the transcript folder"));
    await user.click(button("Refresh"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Refresh failed");
    expect(alert).toHaveTextContent("cannot read the transcript folder");
    expect(screen.getByRole("group", { name: "Usage summary" })).toBeVisible();
    bridge.set("usage", refreshedWorld());
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(bridge.count("usage")).toBe(2);
  });
});

describe("Usage tab, end to end: the OTel receiver", () => {
  it("usage.otel-status: reports the receiver off and Check status reads it again", async () => {
    const user = await openUsage();
    await strip();
    expect(await within(card()).findByText("Off")).toBeVisible();
    expect(within(await strip()).getByText("Off")).toBeVisible();
    expect(within(card()).getByText("none yet")).toBeVisible();
    expect(within(card()).getAllByText("not set")).toHaveLength(5);
    otel.enable(PORT, false);
    await user.click(button("Check status"));
    expect(await within(card()).findByText("Listening")).toBeVisible();
    expect(bridge.count("obs otel status")).toBe(2);
  });

  it("usage.otel-enable: a preview lists the keys and changes nothing, the apply turns the receiver on", async () => {
    const user = await openUsage();
    await within(card()).findByText("Off");
    await user.click(button("Enable…"));
    const box = await screen.findByRole("dialog", {
      name: `Enable the OTel receiver on port ${PORT}?`,
    });
    for (const line of goldenData("obs-otel-enable.preview").actions as string[])
      expect(within(box).getByText(line, { normalizer: (text) => text })).toBeVisible();
    expect(within(box).getByText(/Restart running Claude Code sessions/)).toBeVisible();
    expect(within(box).queryByRole("textbox")).toBeNull();
    expect(bridge.ran()).toContain(`obs otel enable --port ${PORT} --dry-run`);
    expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(0);
    expect(otel.status().enabled).toBe(false);

    await user.click(within(box).getByRole("button", { name: "Enable receiver" }));
    await closeResult(user);
    expect(await within(card()).findByText("Listening")).toBeVisible();
    expect(within(await strip()).getByText("Listening")).toBeVisible();
    expect(within(card()).getByText("Every telemetry key is set")).toBeVisible();
    expect(within(card()).getAllByText("set")).toHaveLength(5);
    expect(within(card()).queryByRole("button", { name: "Enable…" })).toBeNull();
    expect(button("Disable…")).toBeEnabled();
    expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(1);
  });

  it("usage.otel-enable: cancelling the preview applies nothing", async () => {
    const user = await openUsage();
    await within(card()).findByText("Off");
    await user.click(button("Enable…"));
    const box = await screen.findByRole("dialog");
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(0);
    expect(otel.status().enabled).toBe(false);
    expect(within(card()).getByText("Off")).toBeVisible();
  });

  it("usage.otel-enable: a settings conflict from the preview is shown and nothing can be applied", async () => {
    bridge.set(
      `obs otel enable --port ${PORT} --dry-run`,
      failure(
        "conflict",
        "the Claude settings already set OTEL_LOGS_EXPORTER to another value; not overwritten",
      ),
    );
    const user = await openUsage();
    await within(card()).findByText("Off");
    await user.click(button("Enable…"));
    const box = await screen.findByRole("dialog");
    expect(await within(box).findByRole("alert")).toHaveTextContent(
      "already set OTEL_LOGS_EXPORTER",
    );
    expect(within(box).queryByRole("button", { name: "Enable receiver" })).toBeNull();
    expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(0);
  });

  it("usage.otel-disable: a preview names the keys it removes, the apply turns the receiver off", async () => {
    start({ receiver: true });
    const user = await openUsage();
    expect(await within(card()).findByText("Listening")).toBeVisible();
    await user.click(button("Disable…"));
    const box = await screen.findByRole("dialog", { name: "Disable the OTel receiver?" });
    for (const line of goldenData("obs-otel-disable.preview").actions as string[])
      expect(within(box).getByText(line, { normalizer: (text) => text })).toBeVisible();
    expect(
      within(box).getByText(`toolportctl obs otel enable --port ${PORT}`),
    ).toBeVisible();
    expect(bridge.ran()).toContain("obs otel disable --dry-run");
    expect(bridge.count("obs otel disable")).toBe(0);
    expect(otel.status().enabled).toBe(true);

    await user.click(within(box).getByRole("button", { name: "Disable receiver" }));
    await closeResult(user);
    expect(await within(card()).findByText("Off")).toBeVisible();
    expect(within(await strip()).getByText("Off")).toBeVisible();
    expect(within(card()).getAllByText("not set")).toHaveLength(5);
    expect(within(card()).getByRole("button", { name: "Enable…" })).toBeEnabled();
    expect(bridge.count("obs otel disable")).toBe(1);
  });

  it("usage.otel-enable, usage.otel-disable: the receiver goes on and off again", async () => {
    const user = await openUsage();
    await within(card()).findByText("Off");
    await user.click(button("Enable…"));
    await user.click(await screen.findByRole("button", { name: "Enable receiver" }));
    await closeResult(user);
    await within(card()).findByText("Listening");
    await user.click(button("Disable…"));
    await user.click(await screen.findByRole("button", { name: "Disable receiver" }));
    await closeResult(user);
    expect(await within(card()).findByText("Off")).toBeVisible();
    expect(
      bridge
        .ran()
        .filter((line) => line.startsWith("obs otel e") || line.startsWith("obs otel d")),
    ).toEqual([
      `obs otel enable --port ${PORT} --dry-run`,
      `obs otel enable --port ${PORT}`,
      "obs otel disable --dry-run",
      "obs otel disable",
    ]);
  });

  it("usage.otel-status: the status failing says so and Retry reads it again", async () => {
    bridge.set(
      "obs otel status",
      failure("otel_status", "the Claude settings are not valid JSON"),
    );
    const user = await openUsage();
    await strip();
    const alert = await within(card()).findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the receiver status");
    expect(within(await strip()).getByText("Unknown")).toBeVisible();
    bridge.set("obs otel status", () => otel.status());
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await within(card()).findByText("Off")).toBeVisible();
  });
});

describe("Usage tab, end to end: keyboard", () => {
  it("runs Refresh from the keyboard", async () => {
    const user = await openUsage();
    await strip();
    bridge.set("usage", refreshedWorld());
    const folder = screen.getByLabelText(/Transcript folder/);
    await user.click(folder);
    await user.tab();
    expect(button("Refresh")).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(bridge.count("usage")).toBe(1));
  });

  it("opens the Enable preview from the port field with Enter and leaves it with Escape", async () => {
    const user = await openUsage();
    await within(card()).findByText("Off");
    const port = within(card()).getByLabelText("Port");
    await user.click(port);
    await user.keyboard("{Enter}");
    const box = await screen.findByRole("dialog");
    expect(box.contains(document.activeElement)).toBe(true);
    await user.tab();
    expect(box.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(0);
    expect(otel.status().enabled).toBe(false);
    await waitFor(() => expect(port).toHaveFocus());
  });

  it("confirms a Disable from the keyboard", async () => {
    start({ receiver: true });
    const user = await openUsage();
    await within(card()).findByText("Listening");
    button("Disable…").focus();
    await user.keyboard("{Enter}");
    const box = await screen.findByRole("dialog", { name: "Disable the OTel receiver?" });
    const confirm = within(box).getByRole("button", { name: "Disable receiver" });
    confirm.focus();
    await user.keyboard("{Enter}");
    await closeResult(user);
    expect(await within(card()).findByText("Off")).toBeVisible();
  });
});

describe("Usage tab, end to end: nothing it does not name is shown", () => {
  it("renders no unknown member of the usage, status or write envelopes and puts none in an argv", async () => {
    const canary = "CANARY-sk-ant-0123456789";
    const world = usageWorld() as Record<string, unknown>;
    world.leak = canary;
    (world.bySession as Record<string, Record<string, unknown>>)["s-erp-1"].apiKey =
      canary;
    (world.otel as Record<string, unknown>).headers = `Authorization=Bearer ${canary}`;
    bridge.set("usage --no-refresh", world);
    bridge.set("usage", { ...refreshedWorld(), leak: canary });
    bridge.set("obs otel status", () => ({
      ...otel.status(),
      headers: canary,
      settings: {
        ...otel.status().settings,
        env: { OTEL_EXPORTER_OTLP_HEADERS: canary },
      },
    }));
    bridge.set(`obs otel enable --port ${PORT} --dry-run`, {
      ...otel.enable(PORT, true),
      env: { OTEL_EXPORTER_OTLP_HEADERS: canary },
      token: canary,
    });
    bridge.set(`obs otel enable --port ${PORT}`, {
      ...otel.enable(PORT, true),
      token: canary,
    });
    const user = await openUsage();
    await strip();
    await user.click(button("Show the numbers"));
    await user.click(button("Refresh"));
    await screen.findByText(/by Refresh in this window/);
    await user.click(button("Enable…"));
    const box = await screen.findByRole("dialog");
    await within(box).findByText("env.CLAUDE_CODE_ENABLE_TELEMETRY: added");
    expect(document.body.textContent).not.toContain(canary);
    await user.click(within(box).getByRole("button", { name: "Enable receiver" }));
    await waitFor(() => expect(bridge.count(`obs otel enable --port ${PORT}`)).toBe(1));
    await screen.findByRole("dialog", { name: (name) => !name.endsWith("?") });
    expect(document.body.textContent).not.toContain(canary);
    expect(document.body.innerHTML).not.toContain(canary);
    expect(bridge.calls.every((call) => !call.argv.join(" ").includes(canary))).toBe(
      true,
    );
  });
});
