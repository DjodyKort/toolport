import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import {
  hookOf,
  loginsAuthRows,
  loginsHook,
  loginsStatusline,
  signedInRows,
  statuslineOf,
} from "../fixtures/logins";
import { renderTab, wire } from "./harness";
import { HOOK_SNIPPET, STATUSLINE_SNIPPET } from "./model";
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

async function openIntegrations() {
  const view = renderTab("integrations");
  const card = await screen.findByRole("region", { name: "Statusline" });
  await within(card).findByText(loginsStatusline.auth.text);
  return view;
}

const card = (name: string) => screen.getByRole("region", { name });

describe("logins.statusline: auth statusline, live, with the Claude Code snippet", () => {
  it("shows the line the status bar prints right now, its counts and the JSON", async () => {
    await openIntegrations();
    const statusline = card("Statusline");
    expect(within(statusline).getByText(loginsStatusline.auth.text)).toBeInTheDocument();
    const counts = within(statusline).getByRole("list", { name: "Counts" });
    expect(counts).toHaveTextContent("ok: 4");
    expect(counts).toHaveTextContent("needs_reauth: 2");
    const output = within(statusline).getByRole("region", { name: "Statusline output" });
    expect(JSON.parse(output.textContent ?? "")).toEqual(loginsStatusline);
    expect(bridge.ran()).toContain("auth statusline");
  });

  it("summarises the same counts in the strip, without a probe time", async () => {
    await openIntegrations();
    const strip = screen.getByRole("group", { name: "Login summary" });
    expect(within(strip).getByText("4 of 6")).toBeInTheDocument();
    expect(strip).not.toHaveTextContent("Last probe");
  });

  it("shows the snippet that installs it and copies exactly that text", async () => {
    const { user } = await openIntegrations();
    const snippet = within(card("Statusline")).getByRole("region", {
      name: "Statusline snippet",
    });
    expect(snippet).toHaveTextContent(STATUSLINE_SNIPPET);
    expect(STATUSLINE_SNIPPET).toContain("toolportctl auth statusline");
    const copy = within(card("Statusline")).getAllByRole("button", {
      name: "Copy snippet",
    });
    await user.click(copy[0]);
    expect(await navigator.clipboard.readText()).toBe(STATUSLINE_SNIPPET);
    expect(
      await within(card("Statusline")).findByRole("button", { name: "Copied" }),
    ).toBeInTheDocument();
  });

  it("copies the live output as JSON", async () => {
    const { user } = await openIntegrations();
    await user.click(
      within(card("Statusline")).getByRole("button", { name: "Copy output" }),
    );
    expect(JSON.parse(await navigator.clipboard.readText())).toEqual(loginsStatusline);
  });

  it("reads both outputs again on Refresh and shows what changed", async () => {
    const { user } = await openIntegrations();
    bridge.set("auth statusline", statuslineOf(signedInRows));
    bridge.set("auth hook", hookOf(signedInRows));
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await within(card("Statusline")).findByText("auth ok (6)"),
    ).toBeInTheDocument();
    expect(bridge.count("auth statusline")).toBe(2);
    expect(bridge.count("auth hook")).toBe(2);
  });

  it("shows a loading skeleton, then the card", async () => {
    const answer = deferred<unknown>();
    bridge.set("auth statusline", () => answer.promise);
    renderTab("integrations");
    await screen.findByRole("region", { name: "Statusline" });
    expect(screen.getAllByRole("status").length).toBeGreaterThan(0);
    answer.resolve(loginsStatusline);
    expect(await screen.findByText(loginsStatusline.auth.text)).toBeInTheDocument();
  });

  it("shows the failure of one card with Retry and leaves the other card alone", async () => {
    bridge.set(
      "auth statusline",
      ctlReplyFailure("internal", "login cache is unreadable"),
    );
    const { user } = renderTab("integrations");
    const statusline = await screen.findByRole("region", { name: "Statusline" });
    const alert = await within(statusline).findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the statusline output");
    expect(alert).toHaveTextContent("login cache is unreadable");
    expect(
      await within(card("Session start hook")).findByText(/need re-auth/, {
        selector: "p",
      }),
    ).toBeInTheDocument();
    bridge.set("auth statusline", loginsStatusline);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(
      await within(statusline).findByText(loginsStatusline.auth.text),
    ).toBeInTheDocument();
  });

  it("says toolportctl cannot run when the child process is missing, once per card", async () => {
    bridge.set("auth statusline", bridgeDown("toolportctl was not found"));
    bridge.set("auth hook", bridgeDown("toolportctl was not found"));
    const { user, onOpenCommands } = renderTab("integrations");
    await waitFor(() => expect(screen.getAllByRole("alert")).toHaveLength(2));
    for (const alert of screen.getAllByRole("alert"))
      expect(alert).toHaveTextContent("Toolport can't run toolportctl");
    await user.click(screen.getAllByRole("button", { name: "Open doctor" })[0]);
    expect(onOpenCommands).toHaveBeenCalledWith("doctor");
  });

  it("mentions the notifications and where Review leads", async () => {
    await openIntegrations();
    const notes = card("Notifications");
    expect(notes).toHaveTextContent("raises a notification");
    expect(notes).toHaveTextContent("Review opens the Logins tab");
  });
});

describe("logins.hook: auth hook, live, with the SessionStart snippet", () => {
  it("shows what Claude is told at the start of a session when a login is broken", async () => {
    await openIntegrations();
    const hook = card("Session start hook");
    const told = (loginsHook as { hookSpecificOutput: { additionalContext: string } })
      .hookSpecificOutput.additionalContext;
    expect(
      await within(hook).findByText(/need re-auth/, { selector: "p" }),
    ).toBeInTheDocument();
    expect(hook.textContent).toContain(
      "srv-issues: needs_reauth (Sign in to srv-issues again)",
    );
    expect(
      within(hook).getByText("What Claude is told at the start of a session"),
    ).toBeInTheDocument();
    const output = within(hook).getByRole("region", { name: "Hook output" });
    expect(JSON.parse(output.textContent ?? "")).toEqual(loginsHook);
    expect(hook).toHaveTextContent(told.split("\n")[0]);
    expect(bridge.ran()).toContain("auth hook");
  });

  it("says the hook adds nothing when every login works", async () => {
    bridge.set("auth hook", hookOf(signedInRows));
    bridge.set("auth statusline", statuslineOf(signedInRows));
    renderTab("integrations");
    expect(
      await screen.findByText(
        "Every login works, so the hook adds nothing to a session.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("What Claude is told at the start of a session"),
    ).not.toBeInTheDocument();
  });

  it("shows the SessionStart snippet and copies it", async () => {
    const { user } = await openIntegrations();
    const hook = card("Session start hook");
    expect(
      within(hook).getByRole("region", { name: "Session start hook snippet" }),
    ).toHaveTextContent("toolportctl auth hook");
    await user.click(within(hook).getByRole("button", { name: "Copy snippet" }));
    expect(await navigator.clipboard.readText()).toBe(HOOK_SNIPPET);
    expect(HOOK_SNIPPET).toContain("SessionStart");
  });

  it("tolerates an older CLI whose hook output has no hookSpecificOutput", async () => {
    bridge.set("auth hook", { auth: loginsStatusline.auth });
    renderTab("integrations");
    expect(
      await screen.findByText(
        "Every login works, so the hook adds nothing to a session.",
      ),
    ).toBeInTheDocument();
  });

  it("is the end-to-end path of the tab: both outputs, both snippets, one refresh", async () => {
    const { user } = await openIntegrations();
    expect(screen.getAllByRole("button", { name: "Copy snippet" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Copy output" })).toHaveLength(2);
    bridge.set(
      "auth statusline",
      statuslineOf(
        loginsAuthRows.map((row) =>
          row.server === "srv-issues" ? { ...row, state: "revoked" } : row,
        ),
      ),
    );
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(
        within(card("Statusline")).getByText(/1 revoked/, { selector: "p" }),
      ).toBeInTheDocument(),
    );
  });
});
