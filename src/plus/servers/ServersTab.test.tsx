import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { serversWorld } from "../fixtures/servers";
import { renderScreen, wire } from "./harness";
import { clone, createBridge, ctlFailure, deferred, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

const WRITES =
  /^(server (uninstall|install|new|edit)|profile (edit|create|rm)|client (sync|edit|import|direct (add|rm)))/;
const applied = () =>
  bridge.ran().filter((line) => WRITES.test(line) && !line.includes("--dry-run"));

async function select(
  user: ReturnType<typeof renderScreen>["user"],
  region: string,
  name: RegExp,
) {
  const list = await screen.findByRole("region", { name: region });
  await user.click(within(list).getByRole("button", { name }));
}

describe("servers.list: the Servers tab", () => {
  it("groups servers by attention and shows the live state of each", async () => {
    renderScreen();
    const attention = await screen.findByRole("region", { name: "Needs attention" });
    for (const name of ["issue-tracker", "design-files", "acme-erp"]) {
      expect(within(attention).getByText(name)).toBeInTheDocument();
    }
    expect(within(attention).getAllByText("Login needed")).toHaveLength(2);
    expect(within(attention).getByText("Failed")).toBeInTheDocument();
    const connected = screen.getByRole("region", { name: "Connected" });
    expect(within(connected).getByText("docs-search")).toBeInTheDocument();
    expect(within(connected).getByText(/stdio · 14 tools/)).toBeInTheDocument();
    const waiting = screen.getByRole("region", { name: "Not running" });
    expect(within(waiting).getByText("scratch-notes")).toBeInTheDocument();
    expect(within(waiting).getByText("Not in profile")).toBeInTheDocument();
  });

  it("states each server as `server ls`, `status` and the gateway report it, row by row", async () => {
    renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    const expected: Record<string, string> = {
      "docs-search": "stdio · 14 tools Connected",
      "corp-tools": "http · 28 tools Connected",
      "wiki-reader": "http · 31 tools Connected",
      "mail-bridge": "stdio · 9 tools Connected",
      "issue-tracker": "http · Sign-in needed: HTTP 401 invalid_token Login needed",
      "design-files": "http · Sign-in needed: HTTP 401 Unauthorized Login needed",
      "acme-erp": "stdio · The gateway could not start it Failed",
      "scratch-notes": "stdio · Not in the active profile Not in profile",
    };
    for (const server of serversWorld.serverLs.servers) {
      const row = screen.getByRole("button", { name: new RegExp(`${server.name} `) });
      expect(row, server.name).toHaveAccessibleName(
        `${server.name} ${expected[server.name]}`,
      );
    }
    expect(
      screen.getAllByRole("switch", { name: /in the active profile$/ }),
    ).toHaveLength(8);
    expect(
      screen.getByRole("switch", { name: "scratch-notes in the active profile" }),
    ).not.toBeChecked();
    expect(
      screen.getByRole("switch", { name: "acme-erp in the active profile" }),
    ).toBeChecked();
  });

  it("reads and never writes while it only shows", async () => {
    renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    const detail = await screen.findByRole("region", { name: "issue-tracker details" });
    await within(detail).findByText("https://example.test/issue-tracker/mcp");
    expect(applied()).toEqual([]);
    expect(bridge.ran().every((line) => !line.includes("--dry-run"))).toBe(true);
    expect([...new Set(bridge.ran())].sort()).toEqual(
      [
        "client direct ls",
        "client ls",
        "commands",
        "profile ls",
        "server info srv-issues",
        "server ls",
        "status",
      ].sort(),
    );
  });

  it("filters the list by name or transport, and says when nothing matches", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    const search = screen.getByRole("searchbox", { name: "Search servers" });
    await user.type(search, "wiki");
    expect(
      screen.queryByRole("region", { name: "Needs attention" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("wiki-reader")).toBeInTheDocument();
    expect(screen.queryByText("docs-search")).not.toBeInTheDocument();
    await user.clear(search);
    await user.type(search, "zzz");
    expect(screen.getByText("No server matches zzz.")).toBeInTheDocument();
  });

  it("selects the first server that needs attention and follows the one you pick", async () => {
    const { user } = renderScreen();
    expect(
      await screen.findByRole("region", { name: "issue-tracker details" }),
    ).toBeInTheDocument();
    await select(user, "Connected", /corp-tools/);
    const detail = await screen.findByRole("region", { name: "corp-tools details" });
    expect(
      await within(detail).findByText("https://example.test/corp-tools/mcp"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("server info srv-corp");
  });
});

describe("servers.gateway: the strip above the tabs", () => {
  it("shows the gateway, the catalog it built, the logins that need you and the clients", async () => {
    renderScreen();
    const strip = await screen.findByRole("region", { name: "Gateway" });
    const text = strip.textContent ?? "";
    expect(text).toContain("Running");
    expect(text).toContain("daemon, pid 4242, v0.0.0-fixture");
    expect(text).toContain("82 tools");
    expect(text).toContain("from 4 of 7 servers, 3 failed");
    expect(text).toContain("2 need a sign-in");
    expect(text).toContain("issue-tracker, design-files");
    expect(text).toContain("3 managed");
    expect(text).toContain("4 detected, 1 direct entries");
  });

  it("says when the gateway binary is missing or the gateway is not running", async () => {
    const missing = clone(serversWorld.status);
    missing.gateway.present = false;
    bridge.set("status", missing);
    const first = renderScreen();
    expect(
      await within(await screen.findByRole("region", { name: "Gateway" })).findByText(
        "Binary missing",
      ),
    ).toBeInTheDocument();
    first.unmount();

    const stopped = clone(serversWorld.status);
    stopped.gateway.builds = [];
    stopped.gateway.build = null;
    bridge.set("status", stopped);
    renderScreen();
    const strip = await screen.findByRole("region", { name: "Gateway" });
    expect(within(strip).getByText("Not running")).toBeInTheDocument();
    expect(within(strip).getByText("No tool cache")).toBeInTheDocument();
    expect(await screen.findAllByText("Not started")).not.toHaveLength(0);
  });
});

describe("servers.detail: a server, with its login", () => {
  it("shows the launch line, the names of the environment keys and the profiles it is in", async () => {
    const { user } = renderScreen();
    await select(user, "Needs attention", /acme-erp/);
    const detail = await screen.findByRole("region", { name: "acme-erp details" });
    expect(await within(detail).findByText("acme-erp-mcp --stdio")).toBeInTheDocument();
    expect(within(detail).getByText("ERP_API_KEY")).toBeInTheDocument();
    expect(within(detail).getByText("secret, stored in the vault")).toBeInTheDocument();
    expect(within(detail).getByText("ERP_URL")).toBeInTheDocument();
    expect(within(detail).getByText("plain value")).toBeInTheDocument();
    expect(
      within(detail).getByText("Key names only, values stay in the vault."),
    ).toBeInTheDocument();
    expect(within(detail).getByText(/Could not start\./)).toBeInTheDocument();
    expect(
      within(detail).getByRole("switch", { name: "acme-erp in profile Default" }),
    ).toBeChecked();
    expect(
      within(detail).getByRole("switch", { name: "acme-erp in profile Work" }),
    ).not.toBeChecked();
    expect(within(detail).getByText("active")).toBeInTheDocument();
  });

  it("explains a login that is needed and sends you to the Logins tab", async () => {
    const { user } = renderScreen();
    const detail = await screen.findByRole("region", { name: "issue-tracker details" });
    expect(within(detail).getByText("Sign-in needed.")).toBeInTheDocument();
    expect(within(detail).getByText(/HTTP 401 invalid_token/)).toBeInTheDocument();
    await user.click(within(detail).getByRole("button", { name: "Open Logins" }));
    expect(screen.getByRole("tab", { name: "Logins" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(await screen.findByRole("table", { name: "Logins" })).toBeInTheDocument();
  });

  it("says a server outside the active profile is not seen by the clients that follow it", async () => {
    const { user } = renderScreen();
    await select(user, "Not running", /scratch-notes/);
    const detail = await screen.findByRole("region", { name: "scratch-notes details" });
    expect(
      within(detail).getByText(/Not in the active profile, so no client/),
    ).toBeInTheDocument();
  });

  it("shows the error of a server it cannot describe, and retries", async () => {
    bridge.set("server info srv-issues", ctlFailure("not_found", "no such server"));
    const { user } = renderScreen();
    const detail = await screen.findByRole("region", { name: "issue-tracker details" });
    expect(
      await within(detail).findByText("Couldn't load this server"),
    ).toBeInTheDocument();
    expect(within(detail).getByRole("button", { name: "Edit" })).toBeDisabled();
    bridge.set("server info srv-issues", bridge.get("server info srv-docs"));
    await user.click(within(detail).getByRole("button", { name: /Retry/ }));
    await waitFor(() =>
      expect(
        within(detail).queryByText("Couldn't load this server"),
      ).not.toBeInTheDocument(),
    );
  });
});

describe("servers.inspect: the live tools of a server", () => {
  it("connects when asked, lists the tools and filters them", async () => {
    const { user } = renderScreen();
    await select(user, "Connected", /docs-search/);
    const detail = await screen.findByRole("region", { name: "docs-search details" });
    expect(bridge.ran()).not.toContain("inspect srv-docs");
    await user.click(within(detail).getByRole("button", { name: "Inspect live" }));
    const tools = await within(detail).findByRole("list", { name: "Tool list" });
    expect(
      within(tools)
        .getAllByRole("listitem")
        .map((item) => item.querySelector("code")?.textContent),
    ).toEqual(["search_docs", "get_page", "list_spaces"]);
    await user.type(
      within(detail).getByRole("searchbox", { name: "Filter tools" }),
      "page",
    );
    expect(within(tools).getAllByRole("listitem")).toHaveLength(1);
    await user.clear(within(detail).getByRole("searchbox", { name: "Filter tools" }));
    await user.type(
      within(detail).getByRole("searchbox", { name: "Filter tools" }),
      "nothing",
    );
    expect(within(tools).getByText("No tool matches nothing.")).toBeInTheDocument();
  });

  it("tells a server that asks for a login from one that does not start, without a stack of JSON", async () => {
    const { user } = renderScreen();
    const login = await screen.findByRole("region", { name: "issue-tracker details" });
    await user.click(within(login).getByRole("button", { name: "Inspect live" }));
    expect(await within(login).findByText(/asked for a sign-in/)).toBeInTheDocument();
    expect(within(login).getAllByRole("button", { name: "Open Logins" })).toHaveLength(2);

    await select(user, "Needs attention", /acme-erp/);
    const broken = await screen.findByRole("region", { name: "acme-erp details" });
    await user.click(within(broken).getByRole("button", { name: "Inspect live" }));
    expect(await within(broken).findByText(/did not list its tools/)).toBeInTheDocument();
    expect(within(broken).getByText(/could not start acme-erp-mcp/)).toBeInTheDocument();
    expect(
      within(broken).queryByRole("button", { name: "Open Logins" }),
    ).not.toBeInTheDocument();
  });

  it("can be cancelled while it connects", async () => {
    const slow = deferred<unknown>();
    bridge.set("inspect srv-docs", () => slow.promise);
    const { user } = renderScreen();
    await select(user, "Connected", /docs-search/);
    const detail = await screen.findByRole("region", { name: "docs-search details" });
    await user.click(within(detail).getByRole("button", { name: "Inspect live" }));
    await user.click(await within(detail).findByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(bridge.cancelled.length).toBeGreaterThan(0));
    slow.resolve({ profile: null, servers: [{ id: "srv-docs", tools: [] }] });
    expect(await within(detail).findByText(/Cancelled\./)).toBeInTheDocument();
  });
});

describe("servers.remove: removing a server", () => {
  const without = (id: string) => {
    const ls = clone(serversWorld.serverLs);
    ls.servers = ls.servers.filter((server) => server.id !== id);
    return ls;
  };

  async function startRemoval(user: ReturnType<typeof renderScreen>["user"]) {
    await select(user, "Needs attention", /acme-erp/);
    const detail = await screen.findByRole("region", { name: "acme-erp details" });
    await within(detail).findByText("ERP_API_KEY");
    await user.click(within(detail).getByRole("button", { name: "Remove…" }));
    const ask = await screen.findByRole("dialog", { name: "Remove acme-erp" });
    return ask;
  }

  it("previews, asks to type the name, applies once and shows the undo", async () => {
    bridge.after("server uninstall srv-erp", { "server ls": without("srv-erp") });
    const { user } = renderScreen();
    const ask = await startRemoval(user);
    expect(applied()).toEqual([]);
    await user.click(within(ask).getByRole("button", { name: "Preview removal" }));

    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    expect(bridge.ran()).toContain("server uninstall srv-erp --dry-run");
    expect(applied()).toEqual([]);
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(
      within(preview).getByText("Server acme-erp (srv-erp) is removed from the registry"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText("Remove acme-erp from claude-code"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText("Remove secrets from the vault: ERP_API_KEY"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText(/toolportctl server new acme-erp --command acme-erp-mcp/),
    ).toBeInTheDocument();

    const confirm = within(review).getByRole("button", { name: "Remove server" });
    expect(confirm).toBeDisabled();
    const field = within(review).getByRole("textbox", {
      name: /type acme-erp to confirm/i,
    });
    await user.type(field, "acme");
    expect(confirm).toBeDisabled();
    await user.type(field, "{Enter}");
    expect(applied()).toEqual([]);
    await user.type(field, "-erp");
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    const result = await screen.findByRole("dialog", { name: "Remove acme-erp" });
    expect(
      await within(result).findByText("Removed the server acme-erp"),
    ).toBeInTheDocument();
    expect(
      within(result).getByText("Removed secrets from the vault: ERP_API_KEY"),
    ).toBeInTheDocument();
    expect(applied()).toEqual(["server uninstall srv-erp"]);
    const calls = bridge.ran();
    expect(calls.indexOf("server uninstall srv-erp --dry-run")).toBeLessThan(
      calls.indexOf("server uninstall srv-erp"),
    );

    await user.click(within(result).getAllByRole("button", { name: "Close" })[0]);
    await waitFor(() => expect(screen.queryByText("acme-erp")).not.toBeInTheDocument());
    expect(screen.getByRole("tab", { name: /^Servers/ })).toHaveTextContent("7");
  });

  it("applies nothing when you cancel at the confirmation", async () => {
    const { user } = renderScreen();
    const ask = await startRemoval(user);
    await user.click(within(ask).getByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    await within(review).findByRole("region", { name: "Preview" });
    await user.click(within(review).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(applied()).toEqual([]);
    expect(screen.getByText("acme-erp", { selector: "span" })).toBeInTheDocument();
  });

  it("passes --keep-clients and --keep-secrets to the preview and the apply", async () => {
    bridge.set(
      "server uninstall srv-erp --keep-clients --keep-secrets --dry-run",
      bridge.get("server uninstall srv-erp --dry-run"),
    );
    bridge.set(
      "server uninstall srv-erp --keep-clients --keep-secrets",
      bridge.get("server uninstall srv-erp"),
    );
    const { user } = renderScreen();
    const ask = await startRemoval(user);
    await user.click(within(ask).getByRole("checkbox", { name: /--keep-clients/ }));
    await user.click(within(ask).getByRole("checkbox", { name: /--keep-secrets/ }));
    await user.click(within(ask).getByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(
      within(preview).getByText("Client entries are kept (--keep-clients)"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText("Its secrets stay in the vault (--keep-secrets)"),
    ).toBeInTheDocument();
    await user.type(
      within(review).getByRole("textbox", { name: /type acme-erp to confirm/i }),
      "acme-erp",
    );
    await user.click(within(review).getByRole("button", { name: "Remove server" }));
    await screen.findByText("Removed the server acme-erp");
    expect(applied()).toEqual(["server uninstall srv-erp --keep-clients --keep-secrets"]);
  });

  it("shows the CLI's own error when the removal fails, and keeps the server", async () => {
    bridge.set(
      "server uninstall srv-erp",
      ctlFailure("write_failed", "could not write the registry"),
    );
    const { user } = renderScreen();
    const ask = await startRemoval(user);
    await user.click(within(ask).getByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    await within(review).findByRole("region", { name: "Preview" });
    await user.type(
      within(review).getByRole("textbox", { name: /type acme-erp to confirm/i }),
      "acme-erp",
    );
    await user.click(within(review).getByRole("button", { name: "Remove server" }));
    expect(await screen.findByText(/could not write the registry/)).toBeInTheDocument();
    await user.click((await screen.findAllByRole("button", { name: "Close" }))[0]);
    expect(screen.getByRole("tab", { name: /^Servers/ })).toHaveTextContent("8");
  });
});

describe("servers.search and servers.install: adding from the catalog", () => {
  async function openCatalog(user: ReturnType<typeof renderScreen>["user"]) {
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(screen.getByRole("button", { name: "Add server" }));
    return screen.findByRole("dialog", { name: "Add server" });
  }

  it("searches the catalog and lists what each entry needs", async () => {
    const { user } = renderScreen();
    const add = await openCatalog(user);
    await user.type(within(add).getByRole("textbox", { name: "Catalog search" }), "docs");
    await user.click(within(add).getByRole("button", { name: "Search" }));
    const results = await within(add).findByRole("list", { name: "Catalog results" });
    expect(within(results).getAllByRole("listitem")).toHaveLength(2);
    expect(within(results).getByText("docs-index")).toBeInTheDocument();
    expect(within(results).getByText("DOCS_ROOT")).toBeInTheDocument();
    expect(bridge.ran()).toContain("server search docs --limit 20");
  });

  it("falls back to the offline catalog when the network is down", async () => {
    bridge.set(
      "server search docs --limit 20",
      ctlFailure("network", "could not reach the catalog"),
    );
    const { user } = renderScreen();
    const add = await openCatalog(user);
    await user.type(within(add).getByRole("textbox", { name: "Catalog search" }), "docs");
    await user.click(within(add).getByRole("button", { name: "Search" }));
    expect(
      await within(add).findByText(/could not reach the catalog/),
    ).toBeInTheDocument();
    await user.click(within(add).getByRole("button", { name: "Search offline" }));
    expect(
      await within(add).findByRole("list", { name: "Catalog results" }),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("server search docs --limit 20 --offline");
  });

  it("confirms an install with the exact command and a plan, since the command has no preview", async () => {
    const { user } = renderScreen();
    const add = await openCatalog(user);
    await user.type(within(add).getByRole("textbox", { name: "Catalog search" }), "docs");
    await user.click(within(add).getByRole("button", { name: "Search" }));
    await user.click(
      await within(add).findByRole("button", { name: "Install docs-index" }),
    );

    const confirm = await screen.findByRole("dialog", { name: "Install docs-index?" });
    expect(applied()).toEqual([]);
    expect(
      within(confirm).getByText("Add docs-index from the catalog: npx -y docs-index-mcp"),
    ).toBeInTheDocument();
    expect(
      within(confirm).getByText("It needs DOCS_ROOT; set them after installing"),
    ).toBeInTheDocument();
    expect(within(confirm).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl server install docs-index",
    );
    expect(within(confirm).getByText(/no preview/)).toBeInTheDocument();
    expect(bridge.ran().some((line) => line.startsWith("server install"))).toBe(false);

    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    const done = await screen.findByRole("dialog", { name: "Install docs-index" });
    await within(done).findByText(/srv-index/);
    expect(bridge.ran().filter((line) => line.startsWith("server install"))).toEqual([
      "server install docs-index",
    ]);
  });
});

describe("servers.new: a custom server", () => {
  async function openCustom(user: ReturnType<typeof renderScreen>["user"]) {
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(screen.getByRole("button", { name: "Add server" }));
    const add = await screen.findByRole("dialog", { name: "Add server" });
    await user.click(within(add).getByRole("tab", { name: "Custom server" }));
    return add;
  }

  it("asks for what is missing and does not run anything", async () => {
    const { user } = renderScreen();
    const add = await openCustom(user);
    await user.click(within(add).getByRole("button", { name: "Review" }));
    const problems = await within(add).findByRole("alert");
    expect(problems).toHaveTextContent("Give the server a name");
    expect(problems).toHaveTextContent("Give the command that starts it");
    await user.type(within(add).getByRole("textbox", { name: "Name" }), "Docs-Search");
    await user.type(within(add).getByRole("textbox", { name: "Command" }), "node");
    expect(within(add).getByRole("alert")).toHaveTextContent(
      "A server called Docs-Search already exists",
    );
    expect(bridge.ran().some((line) => line.startsWith("server new"))).toBe(false);
  });

  it("shows the command and the plan, then adds the server with one token per value", async () => {
    const { user } = renderScreen();
    const add = await openCustom(user);
    await user.type(within(add).getByRole("textbox", { name: "Name" }), "notes-index");
    await user.type(within(add).getByRole("textbox", { name: "Command" }), "node");
    await user.type(within(add).getByRole("textbox", { name: "Arguments" }), "index.js");
    await user.click(within(add).getByRole("button", { name: "Review" }));

    const confirm = await screen.findByRole("dialog", { name: "Add notes-index?" });
    expect(
      within(confirm).getByText("Add the server notes-index: node index.js"),
    ).toBeInTheDocument();
    expect(within(confirm).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl server new notes-index --command node --arg index.js",
    );
    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    await screen.findByRole("dialog", { name: "Add notes-index" });
    expect(bridge.ran()).toContain(
      "server new notes-index --command node --arg index.js",
    );
    expect(
      bridge
        .ran()
        .some((line) => line.startsWith("server new") && line.includes("--dry-run")),
    ).toBe(false);
  });
});

describe("servers.edit: editing a server", () => {
  it("shows the field it changes as before and after, and applies only that flag", async () => {
    const { user } = renderScreen();
    await select(user, "Connected", /docs-search/);
    const detail = await screen.findByRole("region", { name: "docs-search details" });
    await within(detail).findByText("node server.js --quiet");
    await user.click(within(detail).getByRole("button", { name: "Edit" }));

    const edit = await screen.findByRole("dialog", { name: "Edit docs-search" });
    const review = within(edit).getByRole("button", { name: "Review changes" });
    expect(review).toBeDisabled();
    const command = within(edit).getByRole("textbox", { name: "Command" });
    await user.clear(command);
    await user.type(command, "bun");
    await user.click(review);

    const confirm = await screen.findByRole("dialog", { name: "Edit docs-search?" });
    expect(within(confirm).getByText("Change command")).toBeInTheDocument();
    expect(within(confirm).getByLabelText("Before")).toHaveTextContent("node");
    expect(within(confirm).getByLabelText("After")).toHaveTextContent("bun");
    expect(within(confirm).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl server edit srv-docs --command bun",
    );
    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    await screen.findByRole("dialog", { name: "Edit docs-search" });
    await waitFor(() =>
      expect(bridge.ran()).toContain("server edit srv-docs --command bun"),
    );
  });

  it("says what the CLI cannot do instead of sending it", async () => {
    const { user } = renderScreen();
    await select(user, "Connected", /docs-search/);
    const detail = await screen.findByRole("region", { name: "docs-search details" });
    await within(detail).findByText("node server.js --quiet");
    await user.click(within(detail).getByRole("button", { name: "Edit" }));
    const edit = await screen.findByRole("dialog", { name: "Edit docs-search" });
    await user.clear(within(edit).getByRole("textbox", { name: "Arguments" }));
    expect(within(edit).getByRole("alert")).toHaveTextContent(
      "The arguments cannot be emptied with the CLI",
    );
    expect(within(edit).getByRole("button", { name: "Review changes" })).toBeDisabled();
  });
});

describe("servers.profile-edit: the profile switches of a server", () => {
  it("previews adding a server to a profile with the clients it reaches, then applies", async () => {
    const { user } = renderScreen();
    await select(user, "Connected", /docs-search/);
    const detail = await screen.findByRole("region", { name: "docs-search details" });
    await user.click(
      within(detail).getByRole("switch", { name: "docs-search in profile Work" }),
    );
    const review = await screen.findByRole("dialog", {
      name: "Add docs-search to Work?",
    });
    expect(bridge.ran()).toContain("profile edit work --add-server srv-docs --dry-run");
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(within(preview).getByText("Add srv-docs")).toBeInTheDocument();
    expect(within(preview).getByText(/Used by cursor/)).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Edited the profile Work");
    expect(applied()).toEqual(["profile edit work --add-server srv-docs"]);
  });

  it("uses the active profile for the switch in the list", async () => {
    bridge.set("profile edit default --remove-server srv-docs --dry-run", {
      changed: true,
      dryRun: true,
      id: "default",
      name: "Default",
      notInProfile: [],
      oldName: "Default",
      renamed: false,
      servers: { added: [], after: [], before: ["srv-docs"], removed: ["srv-docs"] },
    });
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(
      screen.getByRole("switch", { name: "docs-search in the active profile" }),
    );
    const review = await screen.findByRole("dialog", {
      name: "Remove docs-search from Default?",
    });
    await within(review).findByRole("region", { name: "Preview" });
    expect(applied()).toEqual([]);
  });
});
