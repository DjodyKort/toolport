import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AuthRow } from "./api";
import { plusAuthRowsFixture } from "./fixtures/authRows";

const { invoke, toastSuccess, toastError } = vi.hoisted(() => ({
  invoke: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("sonner", () => ({ toast: { success: toastSuccess } }));
vi.mock("@/lib/toast", () => ({ toastError }));

const { AuthPanel, AuthRows } = await import("./AuthRows");

const rows: AuthRow[] = [
  {
    server: "beta",
    state: "needs_reauth",
    reason: "invalid_grant",
    since: 1,
    expiresAt: null,
    ttlSecs: null,
    lastProbe: 2,
    fix: {
      action: "reauth",
      server: "beta",
      label: "Sign in to beta again",
      command: "toolportctl auth login beta",
      ipc: null,
    },
  },
  {
    server: "alpha",
    state: "ok",
    reason: "ok",
    since: 1,
    expiresAt: null,
    ttlSecs: null,
    lastProbe: 2,
    fix: null,
  },
];

function routes(handlers: Record<string, (args: unknown) => unknown> = {}) {
  invoke.mockImplementation(async (_cmd: string, payload: unknown) => {
    const { command, args } = payload as { command: string; args: unknown };
    if (command === "plus.auth.rows") return plusAuthRowsFixture;
    const handler = handlers[command];
    if (!handler) throw new Error(`unexpected route ${command}`);
    return handler(args);
  });
}

function calls(command: string) {
  return invoke.mock.calls.filter(
    ([, payload]) => (payload as { command: string }).command === command,
  );
}

beforeEach(() => {
  invoke.mockReset();
  toastSuccess.mockReset();
  toastError.mockReset();
});

describe("AuthRows", () => {
  it("renders state and a fix button only for unhealthy rows", async () => {
    const onFix = vi.fn();
    render(<AuthRows rows={rows} onFix={onFix} />);
    expect(screen.getByText("Needs sign-in")).toBeInTheDocument();
    expect(screen.getByText("Signed in")).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Sign in to beta again" }));
    expect(onFix).toHaveBeenCalledWith(rows[0]);
  });

  it("shows the minutes left and keeps the reason as a tooltip", () => {
    render(<AuthRows rows={plusAuthRowsFixture.rows} />);
    expect(screen.getByText("Expiring (10 min)")).toBeInTheDocument();
    expect(screen.getByText("Needs sign-in")).toHaveAttribute("title", "invalid_grant");
  });

  it("offers a button only where a click can fix something", () => {
    render(<AuthRows rows={plusAuthRowsFixture.rows} />);
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Re-consent access for google-docs",
      "Sign in to figma again",
      "Sign in to linear again",
      "Re-check odoo",
    ]);
    const hint = screen.getByText("Check the OAuth client configuration of notion");
    expect(hint.tagName).toBe("SPAN");
  });

  it("disables every fix while one runs and labels the running row", () => {
    render(<AuthRows rows={plusAuthRowsFixture.rows} busy="figma" />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.every((b) => b.hasAttribute("disabled"))).toBe(true);
    expect(screen.getByRole("button", { name: "Working..." })).toHaveAttribute(
      "aria-busy",
      "true",
    );
  });

  it("renders nothing without rows", () => {
    const { container } = render(<AuthRows rows={[]} />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("AuthPanel", () => {
  it("loads the rows through plus.auth.rows and summarises them", async () => {
    routes();
    render(<AuthPanel />);
    const section = await screen.findByRole("region", { name: "Sign-in health" });
    expect(within(section).getByText("5 need attention")).toBeInTheDocument();
    expect(within(section).getByText("figma")).toBeInTheDocument();
    expect(calls("plus.auth.rows")).toHaveLength(1);
  });

  it("opens Logins & secrets from a button that is only there when the screen is reachable", async () => {
    routes();
    const onOpenLogins = vi.fn();
    const { unmount } = render(<AuthPanel onOpenLogins={onOpenLogins} />);
    const section = await screen.findByRole("region", { name: "Sign-in health" });
    await userEvent
      .setup()
      .click(within(section).getByRole("button", { name: "Open Logins & secrets" }));
    expect(onOpenLogins).toHaveBeenCalledTimes(1);
    unmount();

    render(<AuthPanel />);
    const bare = await screen.findByRole("region", { name: "Sign-in health" });
    expect(
      within(bare).queryByRole("button", { name: "Open Logins & secrets" }),
    ).not.toBeInTheDocument();
  });

  it("says so when every login is fine, and shows nothing when there are no logins", async () => {
    invoke.mockResolvedValueOnce({
      counts: { ...plusAuthRowsFixture.counts },
      rows: [rows[1]],
    });
    const { unmount } = render(<AuthPanel />);
    expect(await screen.findByText("All signed in")).toBeInTheDocument();
    unmount();

    invoke.mockReset();
    invoke.mockResolvedValue({ counts: plusAuthRowsFixture.counts, rows: [] });
    const empty = render(<AuthPanel />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(empty.container).toBeEmptyDOMElement();
  });

  it("stays out of the way when the backend cannot answer", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    const { container } = render(<AuthPanel />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("signs in through plus.auth.login, reports the result and reloads the rows", async () => {
    routes({
      "plus.auth.login": () => ({
        server: "figma",
        name: "figma",
        flow: "browser",
        consentUrl: null,
        signedIn: true,
        message: "Signed in to figma.",
      }),
    });
    render(<AuthPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Sign in to figma again" }),
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith("Signed in to figma."));
    expect(calls("plus.auth.login")).toEqual([
      ["plus_invoke", { command: "plus.auth.login", args: { server: "figma" } }],
    ]);
    await waitFor(() => expect(calls("plus.auth.rows")).toHaveLength(2));
    expect(toastError).not.toHaveBeenCalled();
  });

  it("re-consents revoked logins the same way", async () => {
    routes({ "plus.auth.login": () => ({ message: "Signed in to google-docs." }) });
    render(<AuthPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Re-consent access for google-docs" }),
    );
    await waitFor(() => expect(calls("plus.auth.login")).toHaveLength(1));
    expect(calls("plus.auth.login")[0][1]).toEqual({
      command: "plus.auth.login",
      args: { server: "google-docs" },
    });
  });

  it("runs the route a retry row carries and reloads the rows", async () => {
    routes({ "plus.auth.probe": () => ({ server: "odoo", ran: true, skipped: null }) });
    render(<AuthPanel />);
    await userEvent.click(await screen.findByRole("button", { name: "Re-check odoo" }));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith("Checked odoo again."));
    expect(calls("plus.auth.probe")).toEqual([
      [
        "plus_invoke",
        { command: "plus.auth.probe", args: { server: "odoo", force: true } },
      ],
    ]);
    await waitFor(() => expect(calls("plus.auth.rows")).toHaveLength(2));
  });

  it("shows the refusal text when a sign-in is not possible and frees the buttons", async () => {
    routes({
      "plus.auth.login": () => {
        throw "figma signs in with an API token. Next: toolportctl secret set figma KEY";
      },
    });
    render(<AuthPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Sign in to figma again" }),
    );
    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(toastError.mock.calls[0][0]).toBe(
      "Couldn't fix figma: figma signs in with an API token. Next: toolportctl secret set figma KEY",
    );
    expect(toastSuccess).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "Sign in to figma again" }),
      ).toBeEnabled(),
    );
  });

  it("blocks a second fix while the first one is running", async () => {
    let finish: (value: unknown) => void = () => {};
    routes({
      "plus.auth.login": () => new Promise((resolve) => (finish = resolve)),
    });
    render(<AuthPanel />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Sign in to figma again" }),
    );
    const working = await screen.findByRole("button", { name: "Working..." });
    expect(working).toBeDisabled();
    expect(screen.getByRole("button", { name: "Re-check odoo" })).toBeDisabled();
    finish({ message: "Signed in to figma." });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Re-check odoo" })).toBeEnabled(),
    );
  });
});
