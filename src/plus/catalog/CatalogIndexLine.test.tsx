import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CatalogIndexStatus, CatalogSyncReport } from "../api";

const { status, sync } = vi.hoisted(() => ({ status: vi.fn(), sync: vi.fn() }));
vi.mock("../api", () => ({
  plusCatalogIndexStatus: status,
  plusCatalogIndexSync: sync,
}));

const { CatalogIndexLine } = await import("./CatalogIndexLine");
const { indexSummary } = await import("./model");

const HOUR = 3_600_000;
const iso = (hoursAgo: number) => new Date(Date.now() - hoursAgo * HOUR).toISOString();
const idle = (over: Partial<CatalogIndexStatus> = {}): CatalogIndexStatus => ({
  count: 1123,
  lastSyncedAt: iso(3),
  complete: true,
  running: false,
  ...over,
});
const report: CatalogSyncReport = {
  kind: "incremental",
  pages: 1,
  fetched: 4,
  count: 1127,
  complete: true,
  lastSyncedAt: iso(0),
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function settle(ms = 0) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function refresh() {
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  status.mockReset();
  sync.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("servers.catalog-index: the index line", () => {
  it("says how many servers the index holds and how long ago it synced", async () => {
    status.mockResolvedValue(idle());
    render(<CatalogIndexLine />);
    await settle();
    expect(screen.getByText("Index: 1,123 servers, synced 3 h ago")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("words an index that was never synced or only partly", () => {
    const now = Date.now();
    expect(
      indexSummary(idle({ count: 0, lastSyncedAt: null, complete: false }), now),
    ).toBe("Index: not built yet");
    expect(
      indexSummary(idle({ count: 40, lastSyncedAt: null, complete: false }), now),
    ).toBe("Index: 40 servers, not fully synced yet");
    expect(indexSummary(idle({ count: 1, lastSyncedAt: iso(0) }), now)).toBe(
      "Index: 1 server, synced just now",
    );
    expect(indexSummary(idle({ lastSyncedAt: iso(50) }), now)).toBe(
      "Index: 1,123 servers, synced 2 d ago",
    );
  });

  it("picks up a sync the background watcher starts after the line was drawn", async () => {
    status.mockResolvedValue(idle({ count: 0, lastSyncedAt: null, complete: false }));
    render(<CatalogIndexLine />);
    await settle();
    expect(screen.getByText("Index: not built yet")).toBeInTheDocument();

    status.mockResolvedValue(
      idle({ count: 98, lastSyncedAt: null, complete: false, running: true }),
    );
    await settle(10_000);

    expect(screen.getByRole("status")).toHaveTextContent("98 servers so far");
  });

  it("leaves the line out when the index cannot be read", async () => {
    status.mockRejectedValue(new Error("no backend"));
    const { container } = render(<CatalogIndexLine />);
    await settle();
    expect(container).toBeEmptyDOMElement();
  });
});

describe("servers.catalog-index-refresh: the Refresh button", () => {
  it("runs the sync, shows the count so far while it runs and then the new state", async () => {
    const run = deferred<CatalogSyncReport>();
    const onSynced = vi.fn();
    status.mockResolvedValue(idle());
    sync.mockReturnValue(run.promise);
    render(<CatalogIndexLine onSynced={onSynced} />);
    await settle();

    await refresh();
    expect(sync).toHaveBeenCalledWith(false);
    status.mockResolvedValue(idle({ count: 1200, running: true }));
    await settle(1500);

    expect(screen.getByRole("status")).toHaveTextContent(
      "Syncing the MCP Registry index: 1,200 servers so far",
    );
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();

    status.mockResolvedValue(idle({ count: 1127, lastSyncedAt: iso(0) }));
    await act(async () => run.resolve(report));
    await settle();

    expect(screen.getByText("Index: 1,127 servers, synced just now")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
    expect(onSynced).toHaveBeenCalledTimes(1);
  });

  it("shows why a sync stopped and lets the user try again", async () => {
    status.mockResolvedValue(idle());
    sync.mockRejectedValue(
      "the MCP Registry sync stopped after 2 page(s), the index holds 200 server(s): timed out. Run it again to continue",
    );
    const onSynced = vi.fn();
    render(<CatalogIndexLine onSynced={onSynced} />);
    await settle();

    await refresh();
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("Run it again to continue");
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
    expect(onSynced).not.toHaveBeenCalled();
  });

  it("follows a sync the background watcher started and reports when it ends", async () => {
    const onSynced = vi.fn();
    status.mockResolvedValue(idle({ count: 300, lastSyncedAt: null, running: true }));
    render(<CatalogIndexLine onSynced={onSynced} />);
    await settle();
    expect(screen.getByRole("status")).toHaveTextContent("300 servers so far");
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();

    status.mockResolvedValue(idle({ count: 1123 }));
    await settle(1500);

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("Index: 1,123 servers, synced 3 h ago")).toBeInTheDocument();
    expect(onSynced).toHaveBeenCalledTimes(1);
  });
});
