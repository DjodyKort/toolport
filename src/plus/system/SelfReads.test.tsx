import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import type { WhereAmIResult } from "../types";
import { HowItConnects, WhereAmI } from "./SelfReads";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";

const WHERE = "mcp call where_am_i --args-stdin";
const FLOW = "mcp call flow_diagram --args-stdin";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const whereResult = (change: (result: WhereAmIResult) => void = () => {}) => {
  const data = golden("mcp-call.where_am_i");
  change(data.result);
  return data;
};

const card = async (name: string) => within(await screen.findByRole("group", { name }));

describe("where_am_i (system.where-am-i)", () => {
  it("reads the tool through mcp call with its arguments on stdin and shows the facts", async () => {
    render(<WhereAmI />);
    const where = await card("Where am I");
    expect(await where.findByText("<WORLD>/data")).toBeInTheDocument();
    expect(where.getByText("Readable")).toBeInTheDocument();
    expect(where.getByText("<WORLD>/data/registry.json")).toBeInTheDocument();
    expect(where.getByText("Installed")).toBeInTheDocument();
    expect(where.getByText("<BIN>/toolport-gateway")).toBeInTheDocument();
    expect(where.getByText("No gateway is running")).toBeInTheDocument();
    expect(
      where.getByText("Encrypted file, opened with the key from the environment"),
    ).toBeInTheDocument();
    expect(where.getByText("default")).toBeInTheDocument();
    expect(where.getByText("3 servers, 1 profile")).toBeInTheDocument();
    expect(where.getByText("None tracked")).toBeInTheDocument();
    expect(bridge.ran()).toEqual([WHERE]);
    expect(bridge.stdin(WHERE)).toEqual(["{}"]);
    expect(bridge.ran().join(" ")).not.toContain("--args ");
  });

  it("copies the data folder path", async () => {
    const user = userEvent.setup();
    render(<WhereAmI />);
    await user.click(
      await screen.findByRole("button", { name: "Copy the data folder path" }),
    );
    expect(await navigator.clipboard.readText()).toBe("<WORLD>/data");
  });

  it("says when the registry is missing or cannot be read, and with which error", async () => {
    bridge.set(
      WHERE,
      whereResult((result) => {
        result.registry = {
          error: "registry.json is not valid JSON",
          exists: true,
          path: "/data/registry.json",
          readable: false,
        };
      }),
    );
    render(<WhereAmI />);
    const where = await card("Where am I");
    expect(await where.findByText("Not readable")).toBeInTheDocument();
    expect(where.getByText("registry.json is not valid JSON")).toBeInTheDocument();
  });

  it("shows a registry that does not exist yet and a gateway that is not installed", async () => {
    bridge.set(
      WHERE,
      whereResult((result) => {
        result.registry.exists = false;
        result.registry.readable = false;
        result.gateway.present = false;
        result.secretsBackend = "os-keychain";
      }),
    );
    render(<WhereAmI />);
    const where = await card("Where am I");
    expect(await where.findByText("Not created yet")).toBeInTheDocument();
    expect(where.getByText("Not found")).toBeInTheDocument();
    expect(where.getByText("The operating system keychain")).toBeInTheDocument();
  });

  it("counts the running gateways and lists only the login states that have servers", async () => {
    bridge.set(
      WHERE,
      whereResult((result) => {
        result.gateway.builds = [{}, {}];
        result.auth.counts.ok = 4;
        result.auth.counts.needs_reauth = 2;
      }),
    );
    render(<WhereAmI />);
    const where = await card("Where am I");
    expect(await where.findByText("2 gateways running")).toBeInTheDocument();
    expect(where.getByText("4 signed in")).toBeInTheDocument();
    expect(where.getByText("2 need a new sign-in")).toBeInTheDocument();
    expect(where.queryByText(/revoked/)).toBeNull();
  });

  it("shows a skeleton while it reads", async () => {
    bridge.set(WHERE, () => new Promise(() => {}));
    render(<WhereAmI />);
    expect(await screen.findByRole("status", { name: "Loading" })).toBeInTheDocument();
  });

  it("shows the tool's own message when it fails, with Retry, and nothing else", async () => {
    bridge.set(WHERE, failure("registry_error", "the registry is locked"));
    const user = userEvent.setup();
    render(<WhereAmI />);
    expect(await screen.findByText("the registry is locked")).toBeInTheDocument();
    expect(screen.getByText("Couldn't read where Toolport is")).toBeInTheDocument();
    expect(screen.queryByText("Version")).toBeNull();
    bridge.set(WHERE, whereResult());
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Version")).toBeInTheDocument();
    expect(bridge.count(WHERE)).toBe(2);
  });

  it("reads again on request", async () => {
    const user = userEvent.setup();
    render(<WhereAmI />);
    await screen.findByText("Version");
    await user.click(screen.getByRole("button", { name: "Read where am I again" }));
    await waitFor(() => expect(bridge.count(WHERE)).toBe(2));
  });
});

describe("flow_diagram (system.flow-diagram)", () => {
  it("draws every line of the diagram as boxes joined by arrows, not as raw text", async () => {
    render(<HowItConnects />);
    const flow = await card("How the pieces connect");
    expect(await flow.findByRole("heading", { name: "Data flow" })).toBeInTheDocument();
    const rows = within(flow.getByRole("list", { name: "Flows" })).getAllByRole(
      "listitem",
    );
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent(
      "canonical skills repository goes to transpilers goes to per-client outputs",
    );
    expect(rows[1]).toHaveTextContent("registry (servers, profiles)");
    expect(within(rows[1]).getByText("registry")).toBeInTheDocument();
    expect(within(rows[2]).getByText("encrypted sync bundle")).toBeInTheDocument();
    expect(rows[2]).toHaveTextContent("goes both ways with");
    expect(rows[2]).toHaveTextContent("remote (push and pull)");
    expect(flow.queryByText(/->/)).toBeNull();
    expect(bridge.ran()).toEqual([FLOW]);
    expect(bridge.stdin(FLOW)).toEqual(["{}"]);
  });

  it("keeps a fenced block and a plain line as they are", async () => {
    bridge.set(FLOW, {
      isError: false,
      tier: 1,
      tool: "flow_diagram",
      result: { markdown: "# Notes\n\nNothing moves here.\n```\nA --> B\n```\n" },
    });
    render(<HowItConnects />);
    const flow = await card("How the pieces connect");
    expect(await flow.findByText("Nothing moves here.")).toBeInTheDocument();
    expect(flow.getByLabelText("Diagram source")).toHaveTextContent("A --> B");
  });

  it("says so when the diagram is empty", async () => {
    bridge.set(FLOW, {
      isError: false,
      tier: 1,
      tool: "flow_diagram",
      result: { markdown: "\n" },
    });
    render(<HowItConnects />);
    expect(await screen.findByText("Toolport describes no data flow.")).toBeVisible();
  });

  it("shows the failure with Retry, then the diagram once the tool answers", async () => {
    bridge.set(FLOW, failure("backend_error", "the diagram is unavailable"));
    const user = userEvent.setup();
    render(<HowItConnects />);
    expect(await screen.findByText("the diagram is unavailable")).toBeInTheDocument();
    bridge.set(FLOW, golden("mcp-call.flow_diagram"));
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("list", { name: "Flows" })).toBeInTheDocument();
  });

  it("shows a skeleton while it reads and reads again on request", async () => {
    const user = userEvent.setup();
    let release: (value: unknown) => void = () => {};
    bridge.set(FLOW, () => new Promise((resolve) => (release = resolve)));
    render(<HowItConnects />);
    expect(await screen.findByRole("status", { name: "Loading" })).toBeInTheDocument();
    release(golden("mcp-call.flow_diagram"));
    await screen.findByRole("list", { name: "Flows" });
    bridge.set(FLOW, golden("mcp-call.flow_diagram"));
    await user.click(screen.getByRole("button", { name: "Read the data flow again" }));
    await waitFor(() => expect(bridge.count(FLOW)).toBe(2));
  });
});
