import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import {
  CONSENT_URL,
  loginsAuthRows,
  loginsServerLs,
  loginsStatus,
  signedInRows,
  statusFor,
} from "../fixtures/logins";
import { renderTab, wire } from "./harness";
import {
  bridgeDown,
  createBridge,
  ctlReplyFailure,
  deferred,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

async function openLogins(options: { pollMs?: number } = {}) {
  const view = renderTab("logins", options);
  const table = await screen.findByRole("table", { name: "Logins" });
  return { ...view, table };
}

const rowOf = (table: HTMLElement, name: string) =>
  within(table)
    .getByRole("rowheader", { name: new RegExp(name), hidden: true })
    .closest("tr") as HTMLElement;

const names = (table: HTMLElement) =>
  within(table)
    .getAllByRole("rowheader")
    .map((cell) => cell.querySelector("b")?.textContent);

describe("LoginsTab: the list", () => {
  it("shows a loading skeleton until the status has answered", async () => {
    const status = deferred<unknown>();
    bridge.set("status", () => status.promise);
    renderTab("logins");
    expect(
      await screen.findByRole("status", { name: "Loading logins" }),
    ).toBeInTheDocument();
    status.resolve(loginsStatus);
    expect(await screen.findByRole("table", { name: "Logins" })).toBeInTheDocument();
    expect(
      screen.queryByRole("status", { name: "Loading logins" }),
    ).not.toBeInTheDocument();
  });

  it("lists the broken logins first, each with its type, state, reason and last probe", async () => {
    const { table } = await openLogins();
    expect(names(table)).toEqual([
      "design-files",
      "issue-tracker",
      "corp-tools",
      "wiki-reader",
      "acme-erp",
      "mail-bridge",
    ]);
    const broken = rowOf(table, "issue-tracker");
    expect(within(broken).getByText("OAuth")).toBeInTheDocument();
    expect(within(broken).getByText("Login needed")).toBeInTheDocument();
    expect(within(broken).getByText("HTTP 401 invalid_token")).toBeInTheDocument();
    expect(broken.querySelector("time")).toHaveAttribute("datetime");
    const token = rowOf(table, "mail-bridge");
    expect(within(token).getByText("API token")).toBeInTheDocument();
    expect(within(token).getByText("Signed in")).toBeInTheDocument();
    expect(
      within(screen.getByRole("table", { name: "Logins" }))
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent),
    ).toEqual(["Server", "Type", "State", "Last probe", "Actions"]);
  });

  it("summarises how many logins work and which ones need a sign-in", async () => {
    await openLogins();
    const strip = screen.getByRole("group", { name: "Login summary", hidden: true });
    expect(within(strip).getByText("4 of 6")).toBeInTheDocument();
    expect(strip.textContent).toContain("design-files");
    expect(strip.textContent).toContain("issue-tracker");
    expect(within(strip).getByText("2")).toBeInTheDocument();
    expect(strip.textContent).toContain("Last probe");
  });

  it("offers one fixing action per kind: Sign in for OAuth, Set secret for a token", async () => {
    const { table } = await openLogins();
    expect(
      within(rowOf(table, "issue-tracker")).getByRole("button", {
        name: "Sign in to issue-tracker",
      }),
    ).toBeInTheDocument();
    expect(
      within(rowOf(table, "corp-tools")).getByRole("button", {
        name: "Sign in again to corp-tools",
      }),
    ).toBeInTheDocument();
    const token = rowOf(table, "acme-erp");
    expect(
      within(token).getByRole("button", { name: "Set secret for acme-erp" }),
    ).toBeInTheDocument();
    expect(
      within(token).queryByRole("button", { name: /Sign in/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/2 other servers need no login/)).toBeInTheDocument();
  });

  it("only reads until something is clicked", async () => {
    await openLogins();
    const ran = bridge.ran();
    expect(ran).toContain("status");
    expect(ran).toContain("server ls");
    expect(ran.filter((line) => line.startsWith("server info"))).toHaveLength(
      loginsServerLs.servers.length,
    );
    expect(ran.some((line) => /^(auth probe|auth login|secret )/.test(line))).toBe(false);
  });

  it("reads the login state again on the poll interval", async () => {
    await openLogins({ pollMs: 20 });
    await waitFor(() => expect(bridge.count("status")).toBeGreaterThanOrEqual(3));
  });

  it("reads the state again when Refresh is pressed and shows what changed", async () => {
    const { table, user } = await openLogins();
    bridge.set("status", statusFor(signedInRows));
    await user.click(screen.getByRole("button", { name: "Refresh the list" }));
    await waitFor(() =>
      expect(
        within(rowOf(table, "issue-tracker")).queryByText("Login needed"),
      ).not.toBeInTheDocument(),
    );
    expect(
      within(
        screen.getByRole("group", { name: "Login summary", hidden: true }),
      ).getByText("6 of 6"),
    ).toBeInTheDocument();
  });

  it("shows the CLI's own error with Retry when the status cannot be read", async () => {
    bridge.set("status", ctlReplyFailure("internal", "registry is locked"));
    const { user } = renderTab("logins");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the logins");
    expect(alert).toHaveTextContent("registry is locked");
    bridge.set("status", loginsStatus);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByRole("table", { name: "Logins" })).toBeInTheDocument();
  });

  it("tells a missing toolportctl apart from a failed command and opens the doctor", async () => {
    bridge.set("status", bridgeDown("toolportctl was not found next to the app"));
    const { user, onOpenCommands } = renderTab("logins");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Toolport can't run toolportctl");
    expect(alert).toHaveTextContent("was not found");
    await user.click(within(alert).getByRole("button", { name: "Open doctor" }));
    expect(onOpenCommands).toHaveBeenCalledWith("doctor");
  });

  it("explains an empty registry instead of showing an empty table", async () => {
    bridge.set("server ls", { activeProfile: "default", servers: [] });
    bridge.set("status", statusFor([]));
    renderTab("logins");
    expect(await screen.findByText("No logins to manage")).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Logins" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Probe all" })).toBeDisabled();
  });

  it("keeps the last answer on screen when a refresh fails", async () => {
    const { table, user } = await openLogins();
    bridge.set("status", ctlReplyFailure("internal", "registry is locked"));
    await user.click(screen.getByRole("button", { name: "Refresh the list" }));
    const note = await screen.findByText(/Could not refresh, showing the last answer/);
    expect(note).toHaveTextContent("registry is locked");
    expect(table).toBeInTheDocument();
    expect(names(table)).toContain("issue-tracker");
  });

  it("warns when the gateway is not installed and links to the doctor", async () => {
    bridge.set("status", {
      ...loginsStatus,
      gateway: { present: false, path: null, build: null, builds: [] },
    });
    const { user, onOpenCommands } = renderTab("logins");
    const note = await screen.findByText(/The gateway is not installed/);
    await user.click(
      within(note.closest("[role=status]") as HTMLElement).getByRole("button", {
        name: "Open doctor",
      }),
    );
    expect(onOpenCommands).toHaveBeenCalledWith("doctor");
  });

  it("lists a server whose info could not be read, and says its secrets are missing", async () => {
    bridge.set("server info srv-erp", ctlReplyFailure("internal", "boom"));
    const { table } = await openLogins();
    expect(
      screen.getByText("1 server could not be read, so their secrets are not listed."),
    ).toBeInTheDocument();
    expect(
      within(rowOf(table, "acme-erp")).queryByRole("button", {
        name: "Set secret for acme-erp",
      }),
    ).not.toBeInTheDocument();
  });

  it("can be driven from the keyboard: Tab to a button, Enter opens the sign-in", async () => {
    const { user } = await openLogins();
    const button = screen.getByRole("button", { name: "Sign in to issue-tracker" });
    button.focus();
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", {
      name: "Sign in to issue-tracker",
    });
    expect(dialog.contains(document.activeElement)).toBe(true);
  });
});

describe("logins.signin: auth login from the Logins tab", () => {
  async function startSignIn(server = "issue-tracker") {
    const view = await openLogins();
    await view.user.click(screen.getByRole("button", { name: `Sign in to ${server}` }));
    const dialog = await screen.findByRole("dialog", { name: `Sign in to ${server}` });
    return { ...view, dialog };
  }

  it("runs `auth login <server>` without --no-open and shows the address with Copy address", async () => {
    const { dialog, user } = await startSignIn();
    expect(bridge.ran()).toContain("auth login srv-issues");
    expect(bridge.ran()).not.toContain("auth login srv-issues --no-open");
    expect(
      await within(dialog).findByText("Waiting for the browser…"),
    ).toBeInTheDocument();
    expect(await within(dialog).findByText(CONSENT_URL)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Copy address" }));
    expect(await navigator.clipboard.readText()).toBe(CONSENT_URL);
  });

  it("says it is signed in, reads the status again and flips every row that was fixed", async () => {
    const { dialog, table } = await startSignIn();
    await within(dialog).findByText(CONSENT_URL);
    const before = bridge.count("status");
    bridge.set("status", statusFor(signedInRows));
    bridge.release("auth login srv-issues");
    expect(
      await within(dialog).findByText("Signed in to issue-tracker."),
    ).toBeInTheDocument();
    await waitFor(() => expect(bridge.count("status")).toBeGreaterThan(before));
    await waitFor(() =>
      expect(
        within(rowOf(table, "issue-tracker")).getByText("Signed in"),
      ).toBeInTheDocument(),
    );
  });

  it("'Don't open a browser' stops the run and starts it again with --no-open", async () => {
    const { dialog, user } = await startSignIn();
    await within(dialog).findByText(CONSENT_URL);
    await user.click(
      within(dialog).getByRole("button", { name: "Don't open a browser" }),
    );
    await waitFor(() =>
      expect(bridge.ran()).toContain("auth login srv-issues --no-open"),
    );
    expect(bridge.cancelled).toHaveLength(1);
    expect(
      await within(dialog).findByText(/Open this address in a browser to approve access/),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "Don't open a browser" }),
    ).not.toBeInTheDocument();
  });

  it("Escape does not stop a sign-in that is waiting for the browser", async () => {
    const { dialog, user } = await startSignIn();
    await within(dialog).findByText("Waiting for the browser…");
    await user.keyboard("{Escape}");
    expect(
      screen.getByRole("dialog", { name: "Sign in to issue-tracker" }),
    ).toBeInTheDocument();
    expect(bridge.cancelled).toHaveLength(0);
    expect(
      within(dialog).queryByRole("button", { name: "Close" }),
    ).not.toBeInTheDocument();
  });

  it("Cancel stops the run, says nothing was started and Close ends the dialog", async () => {
    const { dialog, user } = await startSignIn();
    await within(dialog).findByText("Waiting for the browser…");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(
      await within(dialog).findByText("Cancelled. Nothing more was started."),
    ).toBeInTheDocument();
    expect(bridge.cancelled).toHaveLength(1);
    await user.click(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("shows the CLI's failure in the dialog and leaves the row as it was", async () => {
    bridge.set(
      "auth login srv-design",
      ctlReplyFailure("network", "the provider did not answer"),
    );
    const { dialog, table } = await startSignIn("design-files");
    const failed = await within(dialog).findByRole("alert");
    expect(failed).toHaveTextContent("Failed");
    expect(failed).toHaveTextContent("network");
    expect(failed).toHaveTextContent("the provider did not answer");
    expect(
      within(rowOf(table, "design-files")).getByText("Login needed"),
    ).toBeInTheDocument();
  });

  it("sends a token server on to Set secret when the CLI says the sign-in is unsupported", async () => {
    const [erp] = loginsAuthRows.filter((row) => row.server === "srv-erp");
    bridge.set(
      "status",
      statusFor(
        loginsAuthRows.map((row) =>
          row === erp
            ? {
                ...row,
                state: "needs_reauth",
                reason: "no token",
                fix: {
                  action: "reauth",
                  server: "srv-erp",
                  label: "Sign in again",
                  command: "toolportctl auth login srv-erp",
                  ipc: null,
                },
              }
            : row,
        ),
      ),
    );
    const view = await openLogins();
    await view.user.click(screen.getByRole("button", { name: "Sign in to acme-erp" }));
    const dialog = await screen.findByRole("dialog", { name: "Sign in to acme-erp" });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("unsupported");
    expect(
      within(dialog).getByText("This server signs in with a token, not in a browser."),
    ).toBeInTheDocument();
    await view.user.click(within(dialog).getByRole("button", { name: "Set secret…" }));
    const secret = await screen.findByRole("dialog", { name: /^Set ERP_API_KEY$/ });
    expect(within(secret).getByLabelText("Key")).toBeInTheDocument();
  });

  it("is the end-to-end path of the tab: read, sign in, approve, see every list agree", async () => {
    const { dialog, table } = await startSignIn();
    await within(dialog).findByText(CONSENT_URL);
    bridge.set("status", statusFor(signedInRows));
    bridge.release("auth login srv-issues");
    await within(dialog).findByText("Signed in to issue-tracker.");
    await waitFor(() =>
      expect(
        within(
          screen.getByRole("group", { name: "Login summary", hidden: true }),
        ).getByText("6 of 6"),
      ).toBeInTheDocument(),
    );
    expect(
      within(rowOf(table, "issue-tracker")).getByText("Signed in"),
    ).toBeInTheDocument();
    expect(bridge.ran().filter((line) => line.startsWith("auth login"))).toEqual([
      "auth login srv-issues",
    ]);
  });
});

describe("logins.probe: auth probe from the Logins tab", () => {
  it("probes every server ignoring the cache by default and reports what ran", async () => {
    const { user } = await openLogins();
    const before = bridge.count("status");
    await user.click(screen.getByRole("button", { name: "Probe all" }));
    const result = await screen.findByRole("region", { name: "Probe result" });
    expect(
      await within(result).findByText(
        "Probed 6, skipped 0 (a recent result was cached), failed 0.",
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("auth probe --force");
    await waitFor(() => expect(bridge.count("status")).toBeGreaterThan(before));
    await user.click(within(result).getByRole("button", { name: "Dismiss" }));
    expect(
      screen.queryByRole("region", { name: "Probe result" }),
    ).not.toBeInTheDocument();
  });

  it("leaves out --force when the switch is turned off, so a cached result is reused", async () => {
    const { user } = await openLogins();
    await user.click(
      screen.getByRole("switch", { name: "Re-check even if a result is cached" }),
    );
    await user.click(screen.getByRole("button", { name: "Probe all" }));
    expect(
      await screen.findByText(
        "Probed 2, skipped 4 (a recent result was cached), failed 0.",
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("auth probe");
    expect(bridge.ran()).not.toContain("auth probe --force");
  });

  it("probes one server from its row and says which one is running", async () => {
    const run = deferred<unknown>();
    const original = bridge.get("auth probe --server srv-issues --force");
    bridge.set("auth probe --server srv-issues --force", () => run.promise);
    const { table, user } = await openLogins();
    await user.click(
      within(rowOf(table, "issue-tracker")).getByRole("button", {
        name: "Probe issue-tracker",
      }),
    );
    expect(await screen.findByText("Probing srv-issues…")).toBeInTheDocument();
    expect(
      within(rowOf(table, "issue-tracker")).getByRole("button", {
        name: "Probe issue-tracker",
      }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Probe all" })).toBeDisabled();
    run.resolve(original);
    expect(
      await screen.findByText(
        "Probed 1, skipped 0 (a recent result was cached), failed 0.",
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("auth probe --server srv-issues --force");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Probe all" })).toBeEnabled(),
    );
  });

  it("lists the servers a probe failed for, with the reason", async () => {
    const base = bridge.get("auth probe --force") as Record<string, unknown>;
    bridge.set("auth probe --force", {
      ...base,
      failures: [{ server: "srv-mail", error: "timed out after 10s" }],
    });
    const { user } = await openLogins();
    await user.click(screen.getByRole("button", { name: "Probe all" }));
    const list = await screen.findByRole("list", { name: "Probe failures" });
    expect(within(list).getByText("srv-mail")).toBeInTheDocument();
    expect(list).toHaveTextContent("timed out after 10s");
    expect(screen.getByText(/failed 1\./)).toBeInTheDocument();
  });

  it("shows a probe that the CLI refused as a failure with its code", async () => {
    bridge.set("auth probe --force", ctlReplyFailure("internal", "registry is locked"));
    const { user } = await openLogins();
    await user.click(screen.getByRole("button", { name: "Probe all" }));
    const region = await screen.findByRole("region", { name: "Probe result" });
    const failed = await within(region).findByRole("alert");
    expect(failed).toHaveTextContent("registry is locked");
  });

  it("sets a token and probes the server right after, from the row", async () => {
    const { table, user } = await openLogins();
    await user.click(
      within(rowOf(table, "mail-bridge")).getByRole("button", {
        name: "Set secret for mail-bridge",
      }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Set MAIL_TOKEN" });
    await user.type(within(dialog).getByLabelText("New value"), "tok-value-1");
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    expect(await within(dialog).findByText("Saved to the vault")).toBeInTheDocument();
    expect(bridge.calls.find((c) => c.argv[0] === "secret")?.stdin).toBe("tok-value-1");
    await user.click(
      within(dialog).getByRole("button", { name: "Probe mail-bridge now" }),
    );
    await waitFor(() =>
      expect(bridge.ran()).toContain("auth probe --server srv-mail --force"),
    );
    expect(
      await screen.findByRole("region", { name: "Probe result" }),
    ).toBeInTheDocument();
  });

  it("probe is a read: it never opens a confirmation", async () => {
    const { user } = await openLogins();
    await user.click(screen.getByRole("button", { name: "Probe all" }));
    await screen.findByRole("region", { name: "Probe result" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });
});
