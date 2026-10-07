import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { addServer, listStacks, popularCatalog, searchCatalog } from "@/lib/api";
import { plusCatalogIndexStatus, plusCatalogSearchInstant } from "@/plus/api";
import type { CatalogEntry, Registry } from "@/lib/types";
import { CatalogView } from "./CatalogView";

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    addServer: vi.fn(),
    listStacks: vi.fn(),
    popularCatalog: vi.fn(),
    searchCatalog: vi.fn(),
  };
});
vi.mock("@/plus/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/plus/api")>();
  return {
    ...actual,
    plusCatalogSearchInstant: vi.fn(),
    plusCatalogIndexStatus: vi.fn(),
    plusCatalogIndexSync: vi.fn(),
  };
});
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));

const indexed: CatalogEntry = {
  name: "Zqx Indexed Widget",
  description: "Only the local index knows this one.",
  transport: "http",
  command: null,
  args: [],
  url: "https://example.com/mcp",
  envKeys: [],
  source: "registry",
  homepage: null,
  category: "",
};

const registry: Registry = {
  version: 1,
  servers: [],
  profiles: [],
  activeProfileId: null,
};

const IN_FLIGHT = /searching the mcp registry/i;
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(popularCatalog).mockResolvedValue([]);
  vi.mocked(listStacks).mockResolvedValue([]);
  vi.mocked(addServer).mockResolvedValue(registry);
  vi.mocked(searchCatalog).mockResolvedValue({ entries: [] });
  vi.mocked(plusCatalogIndexStatus).mockResolvedValue({
    count: 1123,
    lastSyncedAt: new Date(Date.now() - 2 * 3_600_000).toISOString(),
    complete: true,
    running: false,
  });
});

async function typeQuery(q: string) {
  const user = userEvent.setup();
  render(<CatalogView registry={registry} onAdded={vi.fn()} />);
  await user.type(screen.getByPlaceholderText(/search the mcp registry/i), q);
}

describe("CatalogView with the local registry index", () => {
  it("shows how big the index is and how old", async () => {
    render(<CatalogView registry={registry} onAdded={vi.fn()} />);

    expect(
      await screen.findByText("Index: 1,123 servers, synced 2 h ago"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
  });

  it("renders index hits without calling the registry while the index is fresh", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({
      entries: [indexed],
      liveNeeded: false,
    });

    await typeQuery("zqx");

    expect(await screen.findByText("Zqx Indexed Widget")).toBeInTheDocument();
    await wait(450);
    expect(searchCatalog).not.toHaveBeenCalled();
    expect(screen.queryByText(IN_FLIGHT)).not.toBeInTheDocument();
    expect(screen.queryByText(/no catalog results/i)).not.toBeInTheDocument();
  });

  it("says nothing matched, without a registry call, when a fresh index has no hit", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({
      entries: [],
      liveNeeded: false,
    });

    await typeQuery("zzz-nothing");

    expect(
      await screen.findByText('No catalog results for "zzz-nothing"'),
    ).toBeInTheDocument();
    expect(searchCatalog).not.toHaveBeenCalled();
  });

  it("still asks the registry once the index is empty or stale", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({
      entries: [indexed],
      liveNeeded: true,
    });
    vi.mocked(searchCatalog).mockResolvedValue({ entries: [indexed] });

    await typeQuery("zqx");

    expect(await screen.findByText("Zqx Indexed Widget")).toBeInTheDocument();
    await vi.waitFor(() => expect(searchCatalog).toHaveBeenCalledWith("zqx"));
  });

  it("falls back to the registry when the instant step fails", async () => {
    vi.mocked(plusCatalogSearchInstant).mockRejectedValue(new Error("ipc down"));
    vi.mocked(searchCatalog).mockResolvedValue({ entries: [indexed] });

    await typeQuery("zqx");

    expect(await screen.findByText("Zqx Indexed Widget")).toBeInTheDocument();
    expect(searchCatalog).toHaveBeenCalledWith("zqx");
  });

  it("works without an index line when the index cannot be read", async () => {
    vi.mocked(plusCatalogIndexStatus).mockRejectedValue(new Error("no backend"));
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({
      entries: [indexed],
      liveNeeded: false,
    });

    await typeQuery("zqx");

    expect(await screen.findByText("Zqx Indexed Widget")).toBeInTheDocument();
    expect(screen.queryByText(/^Index:/)).not.toBeInTheDocument();
  });
});
