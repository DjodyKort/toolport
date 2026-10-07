import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { addServer, listStacks, popularCatalog, searchCatalog } from "@/lib/api";
import { plusCatalogSearchInstant } from "@/plus/api";
import type { CatalogEntry, CatalogSearchResult, Registry } from "@/lib/types";
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
  return { ...actual, plusCatalogSearchInstant: vi.fn() };
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

const stripeHelper: CatalogEntry = {
  ...stripe,
  name: "Stripe Payments Helper",
  description: "Community helper for the Stripe API.",
  source: "registry",
  category: "",
};

const IN_FLIGHT = /searching the mcp registry/i;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

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
  vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [] });
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

describe("CatalogView instant results before the registry answers", () => {
  it("shows the instant matches while the registry call is in flight, then merges the registry hits", async () => {
    const registryCall = deferred<CatalogSearchResult>();
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [stripe] });
    vi.mocked(searchCatalog).mockReturnValue(registryCall.promise);

    await typeQuery("stripe");

    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    expect(await screen.findByText(IN_FLIGHT)).toBeInTheDocument();
    expect(plusCatalogSearchInstant).toHaveBeenCalledWith("stripe");
    expect(searchCatalog).toHaveBeenCalledWith("stripe");
    expect(screen.queryByText("Stripe Payments Helper")).not.toBeInTheDocument();

    await act(async () => registryCall.resolve({ entries: [stripe, stripeHelper] }));

    expect(await screen.findByText("Stripe Payments Helper")).toBeInTheDocument();
    expect(screen.getByText("Stripe")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText(IN_FLIGHT)).not.toBeInTheDocument());
  });

  it("puts the in-flight line above the results", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [stripe] });
    vi.mocked(searchCatalog).mockReturnValue(deferred<CatalogSearchResult>().promise);

    await typeQuery("stripe");

    const card = await screen.findByText("Stripe");
    const inFlight = await screen.findByText(IN_FLIGHT);
    expect(
      inFlight.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("debounces the instant step instead of querying on every keystroke", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [stripe] });
    vi.mocked(searchCatalog).mockResolvedValue({ entries: [stripe] });

    await typeQuery("stripe");

    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    expect(plusCatalogSearchInstant).toHaveBeenCalledTimes(1);
    expect(plusCatalogSearchInstant).toHaveBeenCalledWith("stripe");
  });

  it("shows only the in-flight line, not an empty state, while nothing matches instantly", async () => {
    const registryCall = deferred<CatalogSearchResult>();
    vi.mocked(searchCatalog).mockReturnValue(registryCall.promise);

    await typeQuery("zzz-cold-query");

    expect(await screen.findByText(IN_FLIGHT)).toBeInTheDocument();
    expect(screen.queryByText(/no catalog results/i)).not.toBeInTheDocument();
    expect(screen.queryByText("Search failed")).not.toBeInTheDocument();

    await act(async () => registryCall.resolve({ entries: [] }));

    expect(
      await screen.findByText('No catalog results for "zzz-cold-query"'),
    ).toBeInTheDocument();
    expect(screen.queryByText(IN_FLIGHT)).not.toBeInTheDocument();
  });

  it("keeps the instant matches and shows the banner when the registry reports a failure", async () => {
    const registryCall = deferred<CatalogSearchResult>();
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [stripe] });
    vi.mocked(searchCatalog).mockReturnValue(registryCall.promise);

    await typeQuery("stripe");
    expect(await screen.findByText(IN_FLIGHT)).toBeInTheDocument();
    expect(screen.getByText("Stripe")).toBeInTheDocument();

    await act(async () =>
      registryCall.resolve({
        entries: [stripe],
        registryError: { kind: "timeout", message: "timed out reading response" },
      }),
    );

    expect(
      await screen.findByText(
        "The MCP Registry is slow or unavailable right now. Showing curated matches.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Stripe")).toBeInTheDocument();
    expect(screen.queryByText(IN_FLIGHT)).not.toBeInTheDocument();
  });

  it("keeps the instant matches when the registry call itself rejects", async () => {
    vi.mocked(plusCatalogSearchInstant).mockResolvedValue({ entries: [stripe] });
    vi.mocked(searchCatalog).mockRejectedValue(new Error("ipc down"));

    await typeQuery("stripe");

    expect(
      await screen.findByText(
        "Toolport couldn't reach the MCP Registry. Showing curated matches only.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Stripe")).toBeInTheDocument();
  });

  it("ignores a slow registry answer for an earlier query", async () => {
    const user = userEvent.setup();
    const earlier = deferred<CatalogSearchResult>();
    const later = deferred<CatalogSearchResult>();
    vi.mocked(searchCatalog).mockImplementation((q) =>
      q === "str" ? earlier.promise : later.promise,
    );
    render(<CatalogView registry={registry} onAdded={vi.fn()} />);
    const input = screen.getByPlaceholderText(/search the mcp registry/i);

    await user.type(input, "str");
    await waitFor(() => expect(searchCatalog).toHaveBeenCalledWith("str"));
    await user.type(input, "ipe");
    await waitFor(() => expect(searchCatalog).toHaveBeenCalledWith("stripe"));
    await act(async () => later.resolve({ entries: [stripe] }));
    expect(await screen.findByText("Stripe")).toBeInTheDocument();
    await act(async () =>
      earlier.resolve({ entries: [{ ...stripeHelper, name: "Stale Earlier Hit" }] }),
    );

    expect(screen.queryByText("Stale Earlier Hit")).not.toBeInTheDocument();
    expect(screen.getByText("Stripe")).toBeInTheDocument();
  });

  it("does not let a late instant answer replace the registry results", async () => {
    const instantCall = deferred<CatalogSearchResult>();
    vi.mocked(plusCatalogSearchInstant).mockReturnValue(instantCall.promise);
    vi.mocked(searchCatalog).mockResolvedValue({ entries: [stripe, stripeHelper] });

    await typeQuery("stripe");
    expect(await screen.findByText("Stripe Payments Helper")).toBeInTheDocument();

    await act(async () => instantCall.resolve({ entries: [stripe] }));

    expect(screen.getByText("Stripe Payments Helper")).toBeInTheDocument();
  });
});
