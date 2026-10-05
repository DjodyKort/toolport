import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { addServer, listStacks, popularCatalog, searchCatalog } from "@/lib/api";
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
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@/lib/toast", () => ({ toastError: vi.fn() }));

const stripe: CatalogEntry = {
  name: "Stripe",
  description: "Payments, customers, charges, and balances.",
  transport: "http",
  command: null,
  args: [],
  url: "https://mcp.stripe.com",
  envKeys: [],
  source: "curated",
  homepage: null,
  category: "Apps & productivity",
};

const registry: Registry = {
  version: 1,
  servers: [],
  profiles: [],
  activeProfileId: null,
};

async function typeQuery(q: string) {
  const user = userEvent.setup();
  render(<CatalogView registry={registry} onAdded={vi.fn()} />);
  await user.type(screen.getByPlaceholderText(/search the mcp registry/i), q);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(popularCatalog).mockResolvedValue([]);
  vi.mocked(listStacks).mockResolvedValue([]);
  vi.mocked(addServer).mockResolvedValue(registry);
});

describe("CatalogView registry search errors (D-101)", () => {
  it("shows curated hits with a slow-registry banner on a timeout, not the old couldn't-reach message", async () => {
    vi.mocked(searchCatalog).mockResolvedValue({
      entries: [stripe],
      registryError: { kind: "timeout", message: "timed out reading response" },
    });

    await typeQuery("stripe");

    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The MCP Registry is slow or unavailable right now. Showing curated matches.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/couldn't reach the MCP Registry/i),
    ).not.toBeInTheDocument();
  });

  it("shows curated hits with the couldn't-reach banner on a genuine connection failure", async () => {
    vi.mocked(searchCatalog).mockResolvedValue({
      entries: [stripe],
      registryError: { kind: "connectionFailed", message: "connection refused" },
    });

    await typeQuery("stripe");

    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Toolport couldn't reach the MCP Registry. Showing curated matches only.",
      ),
    ).toBeInTheDocument();
  });

  it("blocks with the slow-registry message (not couldn't reach) when a timeout leaves nothing to show", async () => {
    vi.mocked(searchCatalog).mockResolvedValue({
      entries: [],
      registryError: { kind: "timeout", message: "timed out reading response" },
    });

    await typeQuery("zzz-nothing-curated");

    expect(await screen.findByText("Search failed")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The MCP Registry is slow or unavailable right now. Try again in a moment.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/couldn't reach the MCP Registry/i),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("blocks with the couldn't-reach message when a connection failure leaves nothing to show", async () => {
    vi.mocked(searchCatalog).mockResolvedValue({
      entries: [],
      registryError: { kind: "connectionFailed", message: "connection refused" },
    });

    await typeQuery("zzz-nothing-curated");

    expect(await screen.findByText("Search failed")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Toolport couldn't reach the MCP Registry. Check your connection, then retry.",
      ),
    ).toBeInTheDocument();
  });

  it("shows a plain empty state with no banner when there is no registry error", async () => {
    vi.mocked(searchCatalog).mockResolvedValue({ entries: [stripe] });

    await typeQuery("stripe");

    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    expect(screen.queryByText(/MCP Registry is slow/i)).not.toBeInTheDocument();
    expect(
      screen.queryByText(/couldn't reach the MCP Registry/i),
    ).not.toBeInTheDocument();
  });
});
