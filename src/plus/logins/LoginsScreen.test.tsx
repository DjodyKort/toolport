import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import type { AuthRow } from "../api";
import {
  CONSENT_URL,
  hookOf,
  loginsAuthRows,
  signedInRows,
  statusFor,
  statuslineOf,
} from "../fixtures/logins";
import { renderScreen, wire } from "./harness";
import { createBridge, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

const tab = (name: string) => screen.getByRole("tab", { name });

function world(rows: AuthRow[]) {
  bridge.set("status", statusFor(rows));
  bridge.set("auth statusline", statuslineOf(rows));
  bridge.set("auth hook", hookOf(rows));
}

describe("LoginsScreen: the three tabs", () => {
  it("opens on Logins and offers Logins, Secrets and Integrations", async () => {
    renderScreen();
    const tabs = screen.getByRole("tablist", { name: "Logins and secrets sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((t) => t.textContent),
    ).toEqual(["Logins", "Secrets", "Integrations"]);
    expect(tab("Logins")).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("table", { name: "Logins" })).toBeInTheDocument();
  });

  it("can open on another tab", async () => {
    renderScreen({ initialTab: "integrations" });
    expect(tab("Integrations")).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("region", { name: "Statusline" })).toBeInTheDocument();
  });

  it("moves between the tabs with the arrow keys", async () => {
    const { user } = renderScreen();
    await screen.findByRole("table", { name: "Logins" });
    tab("Logins").focus();
    await user.keyboard("{ArrowRight}");
    expect(tab("Secrets")).toHaveAttribute("aria-selected", "true");
    expect(tab("Secrets")).toHaveFocus();
    expect(await screen.findByRole("list", { name: "Secrets" })).toBeInTheDocument();
    await user.keyboard("{ArrowRight}");
    expect(tab("Integrations")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(tab("Logins")).toHaveAttribute("aria-selected", "true");
  });

  it("tells one story on every tab when a login is revoked", async () => {
    world(
      loginsAuthRows.map((row) =>
        row.server === "srv-issues"
          ? {
              ...row,
              state: "revoked",
              reason: "access was revoked",
              fix: {
                action: "reconsent",
                server: "srv-issues",
                label: "Grant access to srv-issues again",
                command: "toolportctl auth login srv-issues",
                ipc: null,
              },
            }
          : row,
      ),
    );
    const { user } = renderScreen();
    const table = await screen.findByRole("table", { name: "Logins" });
    const issues = within(table)
      .getByRole("rowheader", { name: /issue-tracker/ })
      .closest("tr")!;
    expect(within(issues).getByText("Access revoked")).toBeInTheDocument();
    expect(
      within(screen.getByRole("group", { name: "Login summary" })).getByText("4 of 6"),
    ).toBeInTheDocument();

    await user.click(tab("Secrets"));
    const strip = await screen.findByRole("group", { name: "Login summary" });
    expect(within(strip).getByText("4 of 6")).toBeInTheDocument();
    expect(strip).toHaveTextContent("issue-tracker");

    await user.click(tab("Integrations"));
    const statusline = await screen.findByRole("region", { name: "Statusline" });
    expect(
      await within(statusline).findByText(/1 revoked/, { selector: "p" }),
    ).toBeInTheDocument();
    const hook = screen.getByRole("region", { name: "Session start hook" });
    expect(
      await within(hook).findByText(/srv-issues: revoked/, { selector: "p" }),
    ).toBeInTheDocument();
  });

  it("flips Logins, Secrets and Integrations together after a sign-in", async () => {
    const { user } = renderScreen();
    const table = await screen.findByRole("table", { name: "Logins" });
    await user.click(
      within(table).getByRole("button", { name: "Sign in to issue-tracker" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Sign in to issue-tracker",
    });
    await within(dialog).findByText(CONSENT_URL);
    world(signedInRows);
    bridge.release("auth login srv-issues");
    await within(dialog).findByText("Signed in to issue-tracker.");
    await user.click(within(dialog).getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await waitFor(() =>
      expect(
        within(screen.getByRole("group", { name: "Login summary" })).getByText("6 of 6"),
      ).toBeInTheDocument(),
    );

    await user.click(tab("Secrets"));
    expect(
      within(await screen.findByRole("group", { name: "Login summary" })).getByText(
        "6 of 6",
      ),
    ).toBeInTheDocument();

    await user.click(tab("Integrations"));
    expect(await screen.findByText("auth ok (6)")).toBeInTheDocument();
    expect(
      await screen.findByText(
        "Every login works, so the hook adds nothing to a session.",
      ),
    ).toBeInTheDocument();
  });
});
