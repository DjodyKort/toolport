import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { serversWorld } from "../fixtures/servers";
import { renderScreen, tab, wire } from "./harness";
import { clone, createBridge, ctlFailure, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

const WRITES = /^client (sync|edit|import|direct (add|rm))/;
const applied = () =>
  bridge.ran().filter((line) => WRITES.test(line) && !line.includes("--dry-run"));

async function openClients() {
  const view = renderScreen();
  await screen.findByRole("region", { name: "Needs attention" });
  await view.user.click(tab("Clients"));
  const table = await screen.findByRole("table", { name: "Clients" });
  return { ...view, table };
}

async function details(view: Awaited<ReturnType<typeof openClients>>, name: string) {
  await view.user.click(screen.getByRole("button", { name: `Details of ${name}` }));
  return screen.findByRole("dialog", { name: new RegExp(`^${name}`) });
}

const cells = (table: HTMLElement, name: string) => {
  const row = within(table).getByRole("row", { name: new RegExp(`^${name}`) });
  return Array.from(row.querySelectorAll("th, td")).map((cell) =>
    cell.textContent?.trim(),
  );
};

describe("servers.clients: the Clients tab", () => {
  it("lists every client with its gateway state, profile and what it sees", async () => {
    const { table } = await openClients();
    expect(cells(table, "Claude Code")).toEqual([
      "Claude Code managed",
      "Default (active)",
      "7",
      "82 tools from 4 servers",
      "Details",
    ]);
    expect(cells(table, "Claude Desktop")).toEqual([
      "Claude Desktop managed",
      "Research",
      "3",
      "45 tools from 2 servers",
      "Details",
    ]);
    expect(cells(table, "Cursor")).toEqual([
      "Cursor managed",
      "Work",
      "3",
      "59 tools from 2 servers",
      "Details",
    ]);
    expect(cells(table, "Gemini CLI")).toEqual([
      "Gemini CLI not set up",
      "Default (active)",
      "7",
      "Not through Toolport",
      "Details",
    ]);
    expect(
      screen.getByText(/Clients without a profile of their own follow Default\./),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
  });

  it("names the orphans: entries in a client's config that no server owns", async () => {
    await openClients();
    const orphans = screen.getByRole("region", { name: "Orphans" });
    expect(within(orphans).getByText("Cursor")).toBeInTheDocument();
    expect(within(orphans).getByText("legacy-lint")).toBeInTheDocument();
    expect(within(orphans).queryByText(/docs-search/)).not.toBeInTheDocument();
    expect(
      within(orphans).getByText(/An orphan is an entry in a client's own config/),
    ).toBeInTheDocument();
  });

  it("shows an empty state when no client is detected, and no Orphans section", async () => {
    const none = clone(serversWorld.clientLs);
    none.clients = [];
    bridge.set("client ls", none);
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(tab("Clients"));
    expect(await screen.findByText("No clients found")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Orphans" })).not.toBeInTheDocument();
  });

  it("shows the CLI's error with Retry when the clients cannot be read", async () => {
    const good = bridge.get("client ls");
    bridge.set(
      "client ls",
      ctlFailure("detect_failed", "could not read the client configs"),
    );
    const { user } = renderScreen();
    await user.click(tab("Clients"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't load the clients");
    expect(alert).toHaveTextContent("could not read the client configs");
    bridge.set("client ls", good);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByRole("table", { name: "Clients" })).toBeInTheDocument();
  });
});

describe("servers.clients: what a client sees", () => {
  it("lists the servers of the profile the client uses, with the tools each gives it", async () => {
    const view = await openClients();
    const dialog = await details(view, "Cursor");
    const sees = within(dialog).getByRole("region", { name: "What this client sees" });
    expect(sees).toHaveTextContent(
      "59 tools from 2 of 3 servers, through the profile Work.",
    );
    const rows = within(sees)
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    expect(rows).toEqual([
      "corp-tools28 tools",
      "issue-trackerLogin needed",
      "wiki-reader31 tools",
    ]);
  });

  it("says why a client that is not set up sees nothing, and what sync does about it", async () => {
    const view = await openClients();
    const dialog = await details(view, "Gemini CLI");
    expect(
      within(dialog).getByRole("region", { name: "What this client sees" }),
    ).toHaveTextContent(
      "Toolport is not set up in this client yet. Sync adds the toolport entry.",
    );
  });

  it("says a client follows the active profile when it has none of its own", async () => {
    const view = await openClients();
    const dialog = await details(view, "Claude Code");
    expect(
      within(dialog).getByRole("region", { name: "What this client sees" }),
    ).toHaveTextContent(
      "82 tools from 4 of 7 servers, through the profile Default (the active one).",
    );
    expect(
      within(dialog).getByText(
        "It has no profile of its own, so it follows the active profile.",
      ),
    ).toBeInTheDocument();
  });

  it("says a client's config was edited by hand instead of guessing what it sees", async () => {
    const hand = clone(serversWorld.clientLs);
    hand.clients[0].gateway = "customized";
    bridge.set("client ls", hand);
    const view = await openClients();
    const dialog = await details(view, "Claude Code");
    expect(
      within(dialog).getByRole("region", { name: "What this client sees" }),
    ).toHaveTextContent("The toolport entry was edited by hand");
  });
});

describe("servers.client-profile: pointing a client at a profile", () => {
  it("previews the change, then applies it", async () => {
    const view = await openClients();
    const dialog = await details(view, "Gemini CLI");
    await view.user.selectOptions(
      within(dialog).getByRole("combobox", { name: "Profile this client uses" }),
      "work",
    );
    await view.user.click(within(dialog).getByRole("button", { name: "Set profile" }));

    const review = await screen.findByRole("dialog", {
      name: "Point Gemini CLI at Work?",
    });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain(
      "client edit gemini-cli --set-profiles work --dry-run",
    );
    expect(within(preview).getByText("Point Gemini CLI at Work")).toBeInTheDocument();
    expect(
      within(preview).getByText(
        /toolportctl client edit gemini-cli --remove-profile Work/,
      ),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await view.user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Set the profile of Gemini CLI");
    expect(applied()).toEqual(["client edit gemini-cli --set-profiles work"]);
  });

  it("does not offer to set the profile it already has", async () => {
    const view = await openClients();
    const dialog = await details(view, "Cursor");
    expect(within(dialog).getByRole("button", { name: "Set profile" })).toBeDisabled();
  });
});

describe("servers.client-sync: syncing the clients", () => {
  it("previews what sync adds and takes out for every managed client, then applies", async () => {
    const view = await openClients();
    await view.user.click(screen.getByRole("button", { name: /Sync clients/ }));
    const ask = await screen.findByRole("dialog", { name: "Sync clients" });
    await view.user.click(within(ask).getByRole("button", { name: "Preview sync" }));

    const review = await screen.findByRole("dialog", {
      name: "Sync the managed clients?",
    });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain("client sync --dry-run");
    expect(
      within(preview).getByText(
        "cursor: remove the direct entry docs-search (redundant)",
      ),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText("cursor: remove the direct entry legacy-lint (orphan)"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText(
        "claude-code: leaves the direct launcher entries docs-search alone",
      ),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await view.user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Synced the managed clients");
    expect(applied()).toEqual(["client sync"]);
  });

  it("limits the sync to one client and can keep the orphans", async () => {
    bridge.set("client sync --client cursor --keep-orphans --dry-run", {
      dryRun: true,
      clients: [
        {
          client: "cursor",
          gateway: "managed",
          removed: [{ name: "docs-search", reason: "redundant" }],
          kept: ["legacy-lint"],
          direct: [],
          backups: [],
          error: null,
        },
      ],
    });
    const view = await openClients();
    const dialog = await details(view, "Cursor");
    await view.user.click(
      within(dialog).getByRole("button", { name: "Sync this client…" }),
    );
    const ask = await screen.findByRole("dialog", { name: "Sync clients" });
    expect(within(ask).getByRole("combobox", { name: "Clients" })).toHaveValue("cursor");
    await view.user.click(within(ask).getByRole("checkbox", { name: /--keep-orphans/ }));
    await view.user.click(within(ask).getByRole("button", { name: "Preview sync" }));
    const review = await screen.findByRole("dialog", { name: "Sync Cursor?" });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(
      within(preview).getByText("cursor: keeps the orphan entries legacy-lint"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain(
      "client sync --client cursor --keep-orphans --dry-run",
    );
  });

  it("says when every client is already in sync", async () => {
    bridge.set("client sync --dry-run", { dryRun: true, clients: [] });
    const view = await openClients();
    await view.user.click(screen.getByRole("button", { name: /Sync clients/ }));
    const ask = await screen.findByRole("dialog", { name: "Sync clients" });
    await view.user.click(within(ask).getByRole("button", { name: "Preview sync" }));
    expect(await screen.findByText("Every client is in sync")).toBeInTheDocument();
  });
});

describe("servers.client-import: importing direct entries", () => {
  async function openImport() {
    const view = await openClients();
    await view.user.click(
      screen.getByRole("button", { name: "Import the orphans of Cursor" }),
    );
    return {
      ...view,
      ask: await screen.findByRole("dialog", { name: "Import from Cursor" }),
    };
  }

  it("lists the entries of the client and only lets the importable ones be chosen", async () => {
    const { ask } = await openImport();
    const list = await within(ask).findByRole("list", { name: "Direct entries" });
    expect(within(list).getByRole("checkbox", { name: /legacy-lint/ })).toBeEnabled();
    expect(within(list).getByRole("checkbox", { name: /docs-search/ })).toBeDisabled();
    expect(within(list).getByText("Already in the registry")).toBeInTheDocument();
    expect(within(list).getByRole("checkbox", { name: /token-tool/ })).toBeDisabled();
    expect(
      within(list).getByText("Holds an inline credential, so it is not imported"),
    ).toBeInTheDocument();
    expect(within(ask).getByRole("button", { name: "Review import" })).toBeDisabled();
    expect(bridge.ran()).toContain("client import cursor --dry-run");
    expect(applied()).toEqual([]);
  });

  it("previews the import of the chosen entries, then applies it", async () => {
    const { user, ask } = await openImport();
    await user.click(await within(ask).findByRole("checkbox", { name: /legacy-lint/ }));
    await user.click(within(ask).getByRole("button", { name: "Review import" }));
    const review = await screen.findByRole("dialog", {
      name: "Import 1 entry from Cursor?",
    });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain("client import cursor --select legacy-lint --dry-run");
    expect(
      within(preview).getByText("Import legacy-lint into the registry"),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Imported 1 server from Cursor");
    expect(applied()).toEqual(["client import cursor --select legacy-lint"]);
  });

  it("puts the imported servers in a profile when one is named", async () => {
    const data = bridge.get("client import cursor --select legacy-lint --dry-run");
    bridge.set(
      "client import cursor --select legacy-lint --profile Imported --dry-run",
      data,
    );
    const { user, ask } = await openImport();
    await user.click(await within(ask).findByRole("checkbox", { name: /legacy-lint/ }));
    await user.type(
      within(ask).getByRole("combobox", { name: "Put them in a profile" }),
      "Imported",
    );
    await user.click(within(ask).getByRole("button", { name: "Review import" }));
    await screen.findByRole("dialog", { name: "Import 1 entry from Cursor?" });
    expect(bridge.ran()).toContain(
      "client import cursor --select legacy-lint --profile Imported --dry-run",
    );
  });

  it("says when the client has no direct entries to import", async () => {
    bridge.set("client import cursor --dry-run", {
      ...(bridge.get("client import cursor --dry-run") as object),
      direct: [],
    });
    const { ask } = await openImport();
    expect(
      await within(ask).findByText("Cursor has no direct entries to import."),
    ).toBeInTheDocument();
  });

  it("shows the CLI's error when the entries cannot be read", async () => {
    bridge.set(
      "client import cursor --dry-run",
      ctlFailure("read_failed", "the config is not valid JSON"),
    );
    const { ask } = await openImport();
    expect(
      await within(ask).findByText(/the config is not valid JSON/),
    ).toBeInTheDocument();
    expect(within(ask).getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });
});

describe("servers.direct-ls: the direct entries of a client", () => {
  it("counts them in the strip and lists the entry Toolport wrote with its state", async () => {
    const view = await openClients();
    expect(
      within(screen.getByRole("region", { name: "Gateway" })).getByText(
        "4 detected, 1 direct entries",
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("client direct ls");
    const dialog = await details(view, "Claude Code");
    const list = within(dialog).getByRole("list", { name: "Direct launcher entries" });
    expect(list).toHaveTextContent("docs-search");
    expect(within(list).getByText("in sync")).toBeInTheDocument();
  });

  it("words every state a direct entry can be in", async () => {
    const states: Record<string, string> = {
      ok: "in sync",
      stale: "out of date",
      customized: "edited by hand",
      missing: "missing",
      orphan: "server removed",
      unrecorded: "not recorded",
    };
    for (const [state, label] of Object.entries(states)) {
      const world = clone(serversWorld.clientLs);
      world.clients[0].launchers = world.clients[0].launchers.map((row) => ({
        ...(row as object),
        state,
      }));
      bridge.set("client ls", world);
      const view = await openClients();
      const dialog = await details(view, "Claude Code");
      expect(
        within(
          within(dialog).getByRole("list", { name: "Direct launcher entries" }),
        ).getByText(label),
      ).toBeInTheDocument();
      view.unmount();
    }
  });

  it("separates the entries Toolport did not write: orphans and duplicates of a server", async () => {
    const view = await openClients();
    const dialog = await details(view, "Cursor");
    const list = within(dialog).getByRole("list", {
      name: "Entries Toolport did not write",
    });
    const rows = within(list)
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    expect(rows).toEqual(["legacy-lintorphan", "docs-searchduplicate of a server"]);
  });
});

describe("servers.direct-add and servers.direct-rm: direct entries", () => {
  it("adds a direct entry after showing its trade-off, and does not offer a server that has one", async () => {
    const view = await openClients();
    const dialog = await details(view, "Claude Code");
    const choose = within(dialog).getByRole("combobox", {
      name: "Add a direct entry for",
    });
    expect(
      within(choose).queryByRole("option", { name: "docs-search" }),
    ).not.toBeInTheDocument();
    view.unmount();

    const second = await openClients();
    const cursor = await details(second, "Cursor");
    await second.user.selectOptions(
      within(cursor).getByRole("combobox", { name: "Add a direct entry for" }),
      "srv-docs",
    );
    await second.user.click(
      within(cursor).getByRole("button", { name: "Add direct entry" }),
    );
    const review = await screen.findByRole("dialog", {
      name: "Add a direct entry for docs-search in Cursor?",
    });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain(
      "client direct add srv-docs --client cursor --dry-run",
    );
    expect(
      within(preview).getByText(/bypasses the Toolport gateway/),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText(/toolportctl client direct rm srv-docs --client cursor/),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await second.user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Added a direct entry for docs-search in Cursor");
    expect(applied()).toEqual(["client direct add srv-docs --client cursor"]);
  });

  it("removes a direct entry after previewing it, with the command that adds it back", async () => {
    const view = await openClients();
    const dialog = await details(view, "Claude Code");
    await view.user.click(
      within(dialog).getByRole("button", { name: "Remove the direct entry docs-search" }),
    );
    const review = await screen.findByRole("dialog", {
      name: "Remove the direct entry docs-search from Claude Code?",
    });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain(
      "client direct rm srv-docs --client claude-code --dry-run",
    );
    expect(
      within(preview).getByText(
        /toolportctl client direct add srv-docs --client claude-code/,
      ),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await view.user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Removed a direct entry of docs-search in Claude Code");
    expect(applied()).toEqual(["client direct rm srv-docs --client claude-code"]);
  });
});

describe("servers.direct-run: starting a direct entry is a terminal job", () => {
  it("shows the command to copy and a disabled Open in Terminal, and never runs it", async () => {
    const view = await openClients();
    const dialog = await details(view, "Claude Code");
    await view.user.click(within(dialog).getByText("How the client starts it"));
    const line = within(dialog).getByLabelText("Command line");
    expect(line).toHaveTextContent("toolportctl direct run srv-docs");
    const open = within(dialog).getByRole("button", { name: /Open in Terminal/ });
    expect(open).toBeDisabled();
    await view.user.click(within(dialog).getByRole("button", { name: "Copy command" }));
    expect(await navigator.clipboard.readText()).toBe("toolportctl direct run srv-docs");
    expect(bridge.ran().some((argv) => argv.startsWith("direct"))).toBe(false);
  });
});
