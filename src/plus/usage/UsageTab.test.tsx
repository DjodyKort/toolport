import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { UsageTab } from "./UsageTab";
import { exact, tokensOf, type Counts } from "./model";
import {
  bridgeDown,
  createBridge,
  deferred,
  failure,
  goldenData,
  wire,
  type Bridge,
} from "./testkit";
import {
  OTEL_ONLY_MESSAGE,
  emptyUsage,
  usageWorld,
  worldMessages,
  type Msg,
} from "./world";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

const TODAY = "2026-10-04";

function open(props: { today?: string } = {}) {
  const user = userEvent.setup();
  const onOpenCommands = vi.fn();
  const view = render(
    <UsageTab today={props.today ?? TODAY} onOpenCommands={onOpenCommands} />,
  );
  return { ...view, user, onOpenCommands };
}

const strip = () => screen.findByRole("group", { name: "Usage summary" });
const section = (name: string) => screen.getByRole("region", { name });

const sumOf = (messages: Msg[]): Counts => ({
  messages: messages.length,
  input: messages.reduce((n, m) => n + m.input, 0),
  output: messages.reduce((n, m) => n + m.output, 0),
  cacheCreation: messages.reduce((n, m) => n + m.cacheCreation, 0),
  cacheRead: messages.reduce((n, m) => n + m.cacheRead, 0),
});

const everything = () => [...worldMessages(), OTEL_ONLY_MESSAGE];
const since = (day: string) => everything().filter((msg) => msg.day >= day);

describe("usage.refresh: the figures of toolportctl usage", () => {
  it("shows the numbers of the golden envelope exactly", async () => {
    bridge.set("usage --no-refresh", goldenData("usage.cached"));
    open({ today: "2026-10-02" });
    const group = await strip();
    expect(within(group).getByText("Tokens, last 14 days")).toBeInTheDocument();
    expect(within(group).getByText("45.3K")).toHaveAttribute("title", "45,300");
    expect(within(group).getByText("6 messages")).toBeInTheDocument();
    expect(within(group).getByText("92.7%")).toBeInTheDocument();
    expect(within(group).getByText("3")).toBeInTheDocument();

    const cache = section("Cache read and write");
    expect(within(cache).getByText("42,000")).toBeInTheDocument();
    expect(within(cache).getByText("3,000")).toBeInTheDocument();
    expect(within(cache).getByText("60")).toBeInTheDocument();
    expect(within(cache).getByText("240")).toBeInTheDocument();
    expect(within(cache).getByText("14.0 to 1")).toBeInTheDocument();

    const projects = within(section("By project")).getByRole("table");
    expect(within(projects).getByText("demo")).toBeInTheDocument();
    expect(within(projects).getByText("/work/demo")).toBeInTheDocument();
    expect(within(projects).getByText("45,300")).toBeInTheDocument();

    const servers = within(section("By MCP server")).getByRole("table");
    expect(within(servers).getByText("github")).toBeInTheDocument();
    expect(bridge.missing).toEqual([]);
  });

  it("opens on the cheap read and does not re-index", async () => {
    open();
    await strip();
    expect(bridge.ran()).toContain("usage --no-refresh");
    expect(bridge.ran()).not.toContain("usage");
  });

  it("reproduces the numbers of a transcript index worked out another way", async () => {
    open();
    const group = await strip();
    const window = sumOf(since("2026-09-21"));
    expect(group.querySelector(`[title="${exact(tokensOf(window))}"]`)).not.toBeNull();
    expect(
      within(group).getByText(`${exact(window.messages)} messages`),
    ).toBeInTheDocument();
    const calls = everything().reduce((n, msg) => n + msg.tools.length, 0);
    expect(within(group).getByText(exact(calls))).toBeInTheDocument();

    const projects = within(section("By project")).getByRole("table");
    const erp = everything().filter((msg) => msg.cwd === "/work/acme-erp");
    const erpRow = within(projects).getByText("acme-erp").closest("tr")!;
    expect(within(erpRow).getByText(exact(tokensOf(sumOf(erp))))).toBeInTheDocument();
    expect(
      within(erpRow).getByText(exact(new Set(erp.map((msg) => msg.session)).size)),
    ).toBeInTheDocument();

    const models = within(section("By model")).getByRole("table");
    const modelA = sumOf(everything().filter((msg) => msg.model === "claude-a"));
    const rowA = within(models).getByText("claude-a").closest("tr")!;
    expect(within(rowA).getByText(exact(modelA.cacheRead))).toBeInTheDocument();
    expect(within(rowA).getByText(exact(modelA.output))).toBeInTheDocument();
  });

  it("states that a message counts once, keyed by its id, and what was indexed", async () => {
    open();
    await strip();
    const sources = section("Where these numbers come from");
    expect(
      within(sources).getByText(/counted once, keyed by its message\.id/),
    ).toBeInTheDocument();
    expect(
      within(sources).getByText(/the last record of an id wins/),
    ).toBeInTheDocument();
    expect(
      within(sources).getByText(
        `${exact(worldMessages().length)} messages from 4 transcript files`,
      ),
    ).toBeInTheDocument();
    expect(within(sources).getByText("2026-10-03 16:30 UTC")).toBeInTheDocument();
    expect(
      within(sources).getByText(/seen by the receiver, 1 not in the transcripts/),
    ).toBeInTheDocument();
    expect(
      within(sources).getByText(/Not in this window: these are the stored figures/),
    ).toBeInTheDocument();
  });
});

describe("Usage tab: tables", () => {
  it("lists the servers by calls and shows the tools of one on request", async () => {
    const { user } = open();
    await strip();
    const table = within(section("By MCP server")).getByRole("table");
    const rows = (await within(table).findAllByRole("row")).slice(1);
    const names = rows.map((row) => within(row).getAllByRole("cell")[0].textContent);
    expect(names).toEqual(["github", "corp-tools"]);
    expect(within(table).queryByText("mcp__github__list")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show the 2 tools of github" }));
    expect(within(table).getByText("mcp__github__list")).toBeInTheDocument();
    expect(within(table).getByText("mcp__github__get")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Hide the 2 tools of github" }));
    expect(within(table).queryByText("mcp__github__list")).toBeNull();
  });

  it("ranks the sessions by tokens and says it is not a cost", async () => {
    open();
    await strip();
    const sessions = section("Top sessions");
    expect(
      within(sessions).getByText(/ranks by tokens, not by cost/),
    ).toBeInTheDocument();
    const rows = within(within(sessions).getByRole("table")).getAllByRole("row").slice(1);
    const tokens = rows.map((row) =>
      Number(within(row).getAllByRole("cell")[4].textContent!.replace(/,/g, "")),
    );
    expect(tokens).toHaveLength(4);
    expect(tokens).toEqual([...tokens].sort((a, b) => b - a));
    const bySession = (id: string) =>
      tokensOf(sumOf(everything().filter((msg) => msg.session === id)));
    expect(tokens[0]).toBe(
      Math.max(...["s-erp-1", "s-erp-2", "s-repo-1", "s-notes-1"].map(bySession)),
    );
  });

  it("shows the connection failures only when there are some", async () => {
    open();
    await strip();
    const failures = section("MCP connection failures");
    expect(within(failures).getByText("docs-server")).toBeInTheDocument();
    expect(within(failures).getByText("2026-10-02 11:00 UTC")).toBeInTheDocument();
  });

  it("limits a long list and shows all of it on request", async () => {
    const world = usageWorld() as { byMcpServer: Record<string, unknown> };
    for (let i = 0; i < 12; i += 1)
      world.byMcpServer[`extra-${String(i).padStart(2, "0")}`] = {
        calls: 1,
        tools: { [`mcp__extra-${i}__t`]: 1 },
      };
    bridge.set("usage --no-refresh", world);
    const { user } = open();
    const servers = await screen.findByRole("region", { name: "By MCP server" });
    expect(within(servers).getAllByRole("row")).toHaveLength(9);
    await user.click(
      within(servers).getByRole("button", { name: "Show all 14 servers" }),
    );
    expect(within(servers).getAllByRole("row")).toHaveLength(15);
  });

  it("says so when no MCP tool was called", async () => {
    bridge.set("usage --no-refresh", {
      ...(usageWorld() as object),
      byMcpServer: {},
      mcpFailures: [],
    });
    open();
    await strip();
    expect(
      screen.getByText("No MCP tool calls in the indexed transcripts."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "MCP connection failures" })).toBeNull();
  });
});

describe("Usage tab: period and chart", () => {
  it("changes the window of the strip and the chart together", async () => {
    const { user } = open();
    await strip();
    await user.click(screen.getByRole("button", { name: "7 days" }));
    const group = screen.getByRole("group", { name: "Usage summary" });
    const week = sumOf(since("2026-09-28"));
    expect(within(group).getByText("Tokens, last 7 days")).toBeInTheDocument();
    expect(
      within(group).getByText(`${exact(week.messages)} messages`),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: /^Tokens per day, 2026-09-28 to 2026-10-04\./ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "7 days" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("tells the reader what the chart shows, with titled axes and a table of the numbers", async () => {
    const { user } = open();
    await strip();
    const window = since("2026-09-21");
    const chart = screen.getByRole("img", {
      name: /^Tokens per day, 2026-09-21 to 2026-10-04\./,
    });
    expect(chart.getAttribute("aria-label")).toContain(
      `Total ${exact(tokensOf(sumOf(window)))} tokens`,
    );
    expect(
      within(chart as unknown as HTMLElement).getByText("Day (UTC)"),
    ).toBeInTheDocument();
    expect(
      within(chart as unknown as HTMLElement).getByText("Tokens"),
    ).toBeInTheDocument();
    expect(chart.querySelectorAll("rect")).toHaveLength(14);

    await user.click(screen.getByRole("button", { name: "Show the numbers" }));
    const table = screen.getByRole("table", { name: "Tokens per day" });
    expect(within(table).getAllByRole("row")).toHaveLength(15);
    const day = sumOf(window.filter((msg) => msg.day === "2026-10-03"));
    const row = within(table).getByText("2026-10-03").closest("tr")!;
    expect(within(row).getByText(exact(tokensOf(day)))).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Hide the numbers" }));
    expect(screen.queryByRole("table", { name: "Tokens per day" })).toBeNull();
  });

  it("says there is nothing to draw when the window holds no message", async () => {
    open({ today: "2027-03-01" });
    await strip();
    expect(screen.getByText(/No messages in the last 14 days/)).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /^Tokens per day/ })).toBeNull();
  });
});

describe("usage.refresh: Refresh and the transcript folder", () => {
  it("re-indexes with `usage`, says so, and shows the new figures and the time", async () => {
    const reply = deferred<unknown>();
    bridge.set("usage", () => reply.promise);
    const { user } = open();
    await strip();
    expect(
      screen.getByText(/Refresh re-indexes: it reads every new line in your transcripts/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Re-indexing your transcripts…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
    expect(bridge.count("usage")).toBe(1);

    const more = usageWorld() as { totals: Counts; byDay: Record<string, Counts> };
    more.byDay["2026-10-04"] = {
      messages: 7,
      input: 1,
      output: 2,
      cacheCreation: 3,
      cacheRead: 4_000_000,
    };
    reply.resolve(more);
    await waitFor(() =>
      expect(screen.queryByText("Re-indexing your transcripts…")).toBeNull(),
    );
    expect(
      within(screen.getByRole("group", { name: "Usage summary" })).getByText(
        /^\d+\.\d+M$/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/by Refresh in this window/)).toBeInTheDocument();
  });

  it("can stop a refresh that takes long", async () => {
    const reply = deferred<unknown>();
    bridge.set("usage", () => reply.promise);
    const { user } = open();
    await strip();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(await screen.findByText("Cancelling…")).toBeInTheDocument();
    expect(bridge.cancelled).toHaveLength(1);
    reply.resolve(usageWorld());
  });

  it("shows a failed refresh with a way to try again, and keeps the stored figures", async () => {
    bridge.set("usage", failure("usage", "the transcript folder is not readable"));
    const { user } = open();
    await strip();
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Refresh failed");
    expect(alert).toHaveTextContent("the transcript folder is not readable");
    expect(screen.getByRole("group", { name: "Usage summary" })).toBeInTheDocument();
    bridge.set("usage", usageWorld());
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(bridge.count("usage")).toBe(2);
  });

  it("reads another folder with --root, for the cheap read and for Refresh", async () => {
    bridge.set("usage --no-refresh --root /fixture/projects", emptyUsage());
    bridge.set("usage --root /fixture/projects", usageWorld());
    const { user } = open();
    await strip();
    await user.type(
      screen.getByLabelText("Transcript folder (--root)"),
      "/fixture/projects",
    );
    await user.click(screen.getByRole("button", { name: "Use this folder" }));
    expect(await screen.findByText("Nothing indexed yet")).toBeInTheDocument();
    expect(bridge.ran()).toContain("usage --no-refresh --root /fixture/projects");
    await user.click(screen.getAllByRole("button", { name: "Refresh" })[0]);
    await strip();
    expect(bridge.ran()).toContain("usage --root /fixture/projects");
  });
});

describe("Usage tab: states", () => {
  it("shows a loading state while the index is read", async () => {
    const reply = deferred<unknown>();
    bridge.set("usage --no-refresh", () => reply.promise);
    open();
    expect(
      await screen.findByRole("status", { name: "Loading usage" }),
    ).toBeInTheDocument();
    reply.resolve(usageWorld());
    await strip();
    expect(screen.queryByRole("status", { name: "Loading usage" })).toBeNull();
  });

  it("explains that the index is empty until it has run once, and offers Refresh", async () => {
    bridge.set("usage --no-refresh", emptyUsage());
    const { user } = open();
    expect(await screen.findByText("Nothing indexed yet")).toBeInTheDocument();
    expect(
      screen.getByText(/The index stays empty until it runs once/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Usage summary" })).toBeNull();
    expect(
      screen.getByRole("region", { name: "OpenTelemetry receiver" }),
    ).toBeInTheDocument();
    bridge.set("usage", usageWorld());
    await user.click(screen.getAllByRole("button", { name: "Refresh" })[1]);
    await strip();
    expect(screen.queryByText("Nothing indexed yet")).toBeNull();
  });

  it("shows the error of the command with Retry that reads again", async () => {
    bridge.set(
      "usage --no-refresh",
      failure("obs_failed", "the observability index is locked"),
    );
    const { user } = open();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the usage index");
    expect(alert).toHaveTextContent("obs_failed");
    expect(alert).toHaveTextContent("the observability index is locked");
    bridge.set("usage --no-refresh", usageWorld());
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    await strip();
    expect(bridge.count("usage --no-refresh")).toBe(2);
  });

  it("says toolportctl could not run when the bridge is down", async () => {
    bridge.set("usage --no-refresh", bridgeDown("toolportctl is not installed"));
    bridge.set("obs otel status", bridgeDown("toolportctl is not installed"));
    open();
    const alerts = await screen.findAllByRole("alert");
    expect(alerts[0]).toHaveTextContent("Toolport could not run toolportctl");
    expect(
      alerts.some((alert) =>
        /toolportctl is not installed/.test(alert.textContent ?? ""),
      ),
    ).toBe(true);
  });

  it("opens All commands on the obs group", async () => {
    const { user, onOpenCommands } = open();
    await strip();
    await user.click(screen.getByRole("button", { name: "All commands" }));
    expect(onOpenCommands).toHaveBeenCalledWith("obs");
  });
});

describe("Usage tab: the screen shows only what it names", () => {
  it("renders none of the members it does not know, in any envelope", async () => {
    const canary = "CANARY-sk-ant-0123456789";
    const world = usageWorld() as Record<string, unknown>;
    world.leak = canary;
    (world.bySession as Record<string, Record<string, unknown>>)["s-erp-1"].apiKey =
      canary;
    (world.otel as Record<string, unknown>).headers = `Authorization=Bearer ${canary}`;
    bridge.set("usage --no-refresh", world);
    bridge.set("obs otel status", {
      ...goldenData("obs-otel-status"),
      headers: canary,
      settings: {
        ...(goldenData("obs-otel-status").settings as object),
        env: { OTEL_EXPORTER_OTLP_HEADERS: canary },
      },
    });
    const { user } = open();
    await strip();
    await screen.findByText("Not in transcripts");
    await user.click(screen.getByRole("button", { name: "Show the numbers" }));
    expect(document.body.textContent).not.toContain(canary);
    expect(document.body.innerHTML).not.toContain(canary);
    expect(bridge.calls.every((call) => !call.argv.join(" ").includes(canary))).toBe(
      true,
    );
  });
});
