import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { plusCtlCancel, plusCtlResult, plusCtlStart } from "../fixtures/plusCtl";
import { serversWorld } from "../fixtures/servers";
import { renderScreen, tab, wire } from "./harness";
import { ServersScreen } from "./ServersScreen";
import {
  bridgeDown,
  clone,
  createBridge,
  ctlFailure,
  deferred,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

/** The dev browser fixture itself, the one `npm run dev` and the browser smoke use: an argv
 * it has no reply for throws, so a screen that asks for something unregistered fails here. */
function wireDevFixture() {
  const calls: string[] = [];
  mocks.listen.mockReset().mockResolvedValue(() => {});
  mocks.invoke
    .mockReset()
    .mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === "plus_ctl") {
        calls.push((args.argv as string[]).join(" "));
        return plusCtlStart(args.argv as string[]);
      }
      if (command === "plus_ctl_result") return plusCtlResult(args.job as string);
      if (command === "plus_ctl_cancel") return plusCtlCancel(args.job as string);
      throw new Error(`unexpected invoke ${command}`);
    });
  return calls;
}

describe("loading, empty, error and offline states", () => {
  it("shows skeletons while the first answers are on their way, and no error", async () => {
    const slow = deferred<unknown>();
    bridge.set("server ls", () => slow.promise);
    bridge.set("status", () => slow.promise);
    renderScreen();
    expect(
      await screen.findByRole("status", { name: "Loading the gateway state" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("status", { name: "Loading" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getAllByRole("tab")).toHaveLength(7);
    slow.resolve(undefined);
  });

  it("offers to add the first server when the registry has none", async () => {
    const none = clone(serversWorld.serverLs);
    none.servers = [];
    const profiles = clone(serversWorld.profileLs);
    profiles.profiles.forEach((profile) => (profile.servers = []));
    bridge.set("server ls", none);
    bridge.set("profile ls", profiles);
    const { user } = renderScreen();
    expect(await screen.findByText("No servers yet")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Add server/ }));
    expect(await screen.findByRole("dialog", { name: "Add server" })).toBeInTheDocument();
  });

  it("shows the CLI's own error with Retry, and recovers when the retry works", async () => {
    const good = bridge.get("server ls");
    bridge.set(
      "server ls",
      ctlFailure("registry_unreadable", "the registry could not be parsed"),
    );
    const { user } = renderScreen();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't load the servers");
    expect(alert).toHaveTextContent("registry_unreadable");
    expect(alert).toHaveTextContent("the registry could not be parsed");
    expect(
      within(alert).getByRole("button", { name: /Copy diagnostics/ }),
    ).toBeInTheDocument();
    bridge.set("server ls", good);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(
      await screen.findByRole("region", { name: "Needs attention" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("says Toolport could not run toolportctl when the bridge is down", async () => {
    bridge.set("status", bridgeDown("toolportctl was not found next to the app"));
    bridge.set("server ls", bridgeDown("toolportctl was not found next to the app"));
    renderScreen();
    const alerts = await screen.findAllByRole("alert");
    expect(
      alerts.some((alert) =>
        alert.textContent?.includes("Toolport could not run toolportctl"),
      ),
    ).toBe(true);
    expect(
      alerts.some((alert) =>
        alert.textContent?.includes("toolportctl was not found next to the app"),
      ),
    ).toBe(true);
    expect(
      screen.queryByRole("region", { name: "Needs attention" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the last answer on screen and says so when a refresh fails", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const { user } = renderScreen({ pollMs: 1000 });
    await screen.findByRole("region", { name: "Needs attention" });
    bridge.set("status", ctlFailure("gateway_busy", "the gateway did not answer"));
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(
      await screen.findByText(
        /Could not refresh, showing the last answer: the gateway did not answer/,
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Needs attention" })).toBeInTheDocument();
    expect(screen.getByText("docs-search")).toBeInTheDocument();
    bridge.set("status", serversWorld.status);
    await user.click(screen.getAllByRole("button", { name: "Retry" })[0]);
    await waitFor(() =>
      expect(screen.queryByText(/Could not refresh/)).not.toBeInTheDocument(),
    );
  });

  it("shows the gateway strip as an error of its own when only status fails", async () => {
    bridge.set("status", ctlFailure("status_failed", "no status"));
    renderScreen();
    expect(
      await screen.findByText("Couldn't read the gateway state"),
    ).toBeInTheDocument();
  });
});

describe("polling the gateway", () => {
  it("reads the status again and shows a server that has since failed", async () => {
    renderScreen({ pollMs: 20 });
    const connected = await screen.findByRole("region", { name: "Connected" });
    expect(within(connected).getByText("docs-search")).toBeInTheDocument();
    const after = clone(serversWorld.status);
    after.gateway.builds[0].servers[0] = { id: "srv-docs", state: "failed", tools: 0 };
    after.gateway.build = after.gateway.builds[0];
    bridge.set("status", after);
    await waitFor(() => {
      const attention = screen.getByRole("region", { name: "Needs attention" });
      expect(within(attention).getByText("docs-search")).toBeInTheDocument();
    });
    expect(bridge.count("status")).toBeGreaterThan(1);
    expect(bridge.count("server ls")).toBe(1);
  });

  it("does not poll while the page is hidden, and polls again when it is shown", async () => {
    renderScreen({ pollMs: 15 });
    await screen.findByRole("region", { name: "Needs attention" });
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    await new Promise((resolve) => setTimeout(resolve, 60));
    const frozen = bridge.count("status");
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(bridge.count("status")).toBe(frozen);
    hidden.mockReturnValue(false);
    await waitFor(() => expect(bridge.count("status")).toBeGreaterThan(frozen));
  });

  it("stops polling when the screen is closed", async () => {
    const view = renderScreen({ pollMs: 15 });
    await screen.findByRole("region", { name: "Needs attention" });
    view.unmount();
    await new Promise((resolve) => setTimeout(resolve, 60));
    const frozen = bridge.count("status");
    await new Promise((resolve) => setTimeout(resolve, 120));
    expect(bridge.count("status")).toBe(frozen);
  });
});

describe("the dev fixture serves every tab, one end-to-end walk each", () => {
  it("Servers: finds the server that fails, reads why, and previews its removal", async () => {
    const calls = wireDevFixture();
    const user = userEvent.setup();
    render(<ServersScreen onOpenCommands={() => {}} pollMs={0} />);
    const attention = await screen.findByRole("region", { name: "Needs attention" });
    await user.click(within(attention).getByRole("button", { name: /acme-erp/ }));
    const detail = await screen.findByRole("region", { name: "acme-erp details" });
    expect(within(detail).getByText(/Could not start\./)).toBeInTheDocument();
    await within(detail).findByText("ERP_API_KEY");
    await user.click(within(detail).getByRole("button", { name: "Remove…" }));
    await user.click(await screen.findByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    await within(review).findByRole("region", { name: "Preview" });
    expect(within(review).getByRole("button", { name: "Remove server" })).toBeDisabled();
    expect(calls).toContain("server uninstall srv-erp --dry-run");
    expect(calls).not.toContain("server uninstall srv-erp");
  });

  it("Profiles: inspects a profile with a login-needing server and gets partial results", async () => {
    wireDevFixture();
    const user = userEvent.setup();
    render(<ServersScreen onOpenCommands={() => {}} pollMs={0} />);
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(tab("Profiles"));
    await user.click(await screen.findByRole("button", { name: "Inspect Work" }));
    const dialog = await screen.findByRole("dialog", { name: "Inspect Work" });
    expect(
      await within(dialog).findByText(
        /2 of 3 servers answered with 4 tools, 1 need a login\./,
      ),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("Needs a login")).toBeInTheDocument();
  });

  it("Clients: shows what a client sees and previews a sync", async () => {
    wireDevFixture();
    const user = userEvent.setup();
    render(<ServersScreen onOpenCommands={() => {}} pollMs={0} />);
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(tab("Clients"));
    await user.click(await screen.findByRole("button", { name: "Details of Cursor" }));
    const dialog = await screen.findByRole("dialog", { name: /^Cursor/ });
    expect(
      within(dialog).getByRole("region", { name: "What this client sees" }),
    ).toHaveTextContent("59 tools from 2 of 3 servers");
    await user.click(within(dialog).getByRole("button", { name: "Sync this client…" }));
    await user.click(await screen.findByRole("button", { name: "Preview sync" }));
    await screen.findByRole("dialog", { name: "Sync Cursor?" }).catch(() => null);
  });

  it("Health: shows the facts, the checks and the fix the CLI names", async () => {
    const calls = wireDevFixture();
    const user = userEvent.setup();
    render(<ServersScreen onOpenCommands={() => {}} pollMs={0} />);
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(tab("Health"));
    expect(
      await screen.findByRole("list", { name: "Doctor checks" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run skills sync" })).toBeInTheDocument();
    expect(calls).toContain("doctor");
  });
});

describe("the classic view", () => {
  it("offers the upstream Servers page from the Servers tab and opens it on click", async () => {
    const { user, onOpenClassic } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(screen.getByRole("button", { name: "Classic view" }));
    expect(onOpenClassic).toHaveBeenCalledTimes(1);
  });

  it("is only on the Servers tab, where the health filter used to be", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    for (const name of ["Profiles", "Clients", "Health"]) {
      await user.click(tab(name));
      expect(screen.queryByRole("button", { name: "Classic view" })).toBeNull();
    }
    await user.click(tab("Servers"));
    expect(screen.getByRole("button", { name: "Classic view" })).toBeInTheDocument();
  });

  it("is left out when the screen is not given a way to open it", async () => {
    wire(mocks, bridge);
    render(<ServersScreen onOpenCommands={() => {}} pollMs={0} />);
    await screen.findByRole("region", { name: "Needs attention" });
    expect(screen.queryByRole("button", { name: "Classic view" })).toBeNull();
  });
});

describe("accessibility", () => {
  const INTERACTIVE = [
    "button",
    "switch",
    "checkbox",
    "textbox",
    "searchbox",
    "combobox",
    "tab",
  ] as const;

  function unnamed() {
    return INTERACTIVE.flatMap((role) =>
      screen
        .queryAllByRole(role)
        .filter((element) => {
          try {
            expect(element).toHaveAccessibleName();
            return false;
          } catch {
            return true;
          }
        })
        .map((element) => `${role}: ${element.outerHTML.slice(0, 80)}`),
    );
  }

  it("names every control on every tab", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await screen.findByRole("region", { name: "issue-tracker details" });
    expect(unnamed()).toEqual([]);
    for (const name of ["Profiles", "Clients", "Health"]) {
      await user.click(tab(name));
      await screen.findByRole("tabpanel");
      await waitFor(() =>
        expect(
          screen.queryByRole("status", { name: /Loading|Running doctor/ }),
        ).not.toBeInTheDocument(),
      );
      expect(unnamed(), name).toEqual([]);
    }
  });

  it("tells the servers apart by name, not by the colour of a badge", async () => {
    renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    for (const name of ["issue-tracker", "acme-erp", "scratch-notes", "docs-search"]) {
      const row = screen.getByRole("button", { name: new RegExp(`${name} `) });
      expect(row.textContent).toMatch(/Login needed|Failed|Not in profile|Connected/);
    }
    expect(screen.getByRole("button", { name: /issue-tracker/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("moves between the tabs with the arrow keys", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    tab("Servers").focus();
    await user.keyboard("{ArrowRight}");
    expect(tab("Profiles")).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("list", { name: "Profiles" })).toBeInTheDocument();
    await user.keyboard("{ArrowRight}");
    expect(tab("Clients")).toHaveAttribute("aria-selected", "true");
  });

  it("keeps focus in a dialog, closes it with Escape and gives focus back", async () => {
    const { user } = renderScreen();
    const detail = await screen.findByRole("region", { name: "issue-tracker details" });
    const remove = within(detail).getByRole("button", { name: "Remove…" });
    await user.click(remove);
    const dialog = await screen.findByRole("dialog", { name: "Remove issue-tracker" });
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() => expect(remove).toHaveFocus());
  });

  it("never confirms a destructive write with Enter before the name is typed", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "issue-tracker details" });
    await user.click(screen.getByRole("button", { name: /acme-erp/ }));
    const detail = await screen.findByRole("region", { name: "acme-erp details" });
    await user.click(within(detail).getByRole("button", { name: "Remove…" }));
    await user.click(await screen.findByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    await within(review).findByRole("region", { name: "Preview" });
    const field = within(review).getByRole("textbox", {
      name: /type acme-erp to confirm/i,
    });
    await user.click(field);
    await user.keyboard("{Enter}");
    expect(bridge.ran()).not.toContain("server uninstall srv-erp");
    expect(screen.getByRole("dialog", { name: "Remove acme-erp?" })).toBeInTheDocument();
  });
});

describe("no secret value reaches the screen or the bridge", () => {
  const CANARY = "CANARY-secret-value-5d21";

  it("never shows a value, even if the CLI were to send one, and never sends one", async () => {
    const info = clone(
      bridge.get("server info srv-erp") as { env: Array<Record<string, unknown>> },
    );
    info.env = info.env.map((row) => (row.secret ? { ...row, value: CANARY } : row));
    bridge.set("server info srv-erp", info);
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "issue-tracker details" });
    await user.click(screen.getByRole("button", { name: /acme-erp/ }));
    const detail = await screen.findByRole("region", { name: "acme-erp details" });
    await within(detail).findByText("ERP_API_KEY");
    await user.click(within(detail).getByRole("button", { name: "Edit" }));
    const edit = await screen.findByRole("dialog", { name: "Edit acme-erp" });
    expect(edit).toHaveAccessibleDescription(/Environment values are not shown here/);
    expect(document.body.textContent).not.toContain(CANARY);
    for (const input of document.querySelectorAll("input, textarea, select")) {
      expect((input as HTMLInputElement).value).not.toContain(CANARY);
    }
    await user.click(within(edit).getByRole("button", { name: "Cancel" }));
    await user.click(within(detail).getByRole("button", { name: "Remove…" }));
    await user.click(await screen.findByRole("button", { name: "Preview removal" }));
    const review = await screen.findByRole("dialog", { name: "Remove acme-erp?" });
    await within(review).findByRole("region", { name: "Preview" });
    expect(document.body.textContent).not.toContain(CANARY);
    expect(review.textContent).toContain("ERP_API_KEY");
    expect(JSON.stringify(bridge.calls)).not.toContain(CANARY);
    expect(bridge.calls.every((call) => call.stdin === null)).toBe(true);
  });

  it("has no field that takes a secret: those are set on the Secrets tab", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(screen.getByRole("button", { name: "Add server" }));
    const add = await screen.findByRole("dialog", { name: "Add server" });
    await user.click(within(add).getByRole("tab", { name: "Custom server" }));
    expect(document.querySelectorAll("input[type=password]")).toHaveLength(0);
    expect(
      within(add).queryByLabelText(/secret|token|password|api key/i),
    ).not.toBeInTheDocument();
  });
});
