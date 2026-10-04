import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { commandsServed, serversWorld } from "../fixtures/servers";
import { renderScreen, tab, wire } from "./harness";
import { clone, createBridge, ctlFailure, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

async function openHealth() {
  const view = renderScreen();
  await screen.findByRole("region", { name: "Needs attention" });
  await view.user.click(tab("Health"));
  const checks = await screen.findByRole("list", { name: "Doctor checks" });
  return { ...view, checks };
}

const healthyDoctor = () => ({
  healthy: true,
  checks: serversWorld.doctor.checks.map((row) => ({
    ...row,
    status: "ok",
    detail: "fine",
  })),
});

describe("servers.health: status and doctor as a health panel", () => {
  it("shows what `status` reports as facts, and the 81 tools of the self-management MCP", async () => {
    await openHealth();
    const facts = screen.getByRole("group", { name: "Facts" });
    const text = facts.textContent ?? "";
    expect(text).toContain("8 servers, 3 profiles");
    expect(text).toContain("encrypted-file");
    expect(text).toContain("Found");
    expect(text).toContain("/fixture/bin/toolport-gateway");
    expect(text).toContain(`${commandsServed.tools.length} tools`);
    expect(text).toContain("2 need a sign-in");
  });

  it("lists every doctor check with its status and detail", async () => {
    const { checks } = await openHealth();
    const rows = within(checks).getAllByRole("listitem");
    expect(rows).toHaveLength(7);
    expect(rows[0]).toHaveTextContent("Data folder");
    expect(rows[0]).toHaveTextContent("OK");
    expect(rows[0]).toHaveTextContent("/fixture/data is writable");
    expect(rows[6]).toHaveTextContent("Skills");
    expect(rows[6]).toHaveTextContent("Warning");
    expect(rows[6]).toHaveTextContent("3 of 12 deployed skill files would be rejected");
    expect(bridge.ran()).toContain("doctor");
  });

  it("offers a fix for the logins that need you and one for every check that is not ok", async () => {
    const { user } = await openHealth();
    const fixes = screen.getByRole("region", { name: "Fixes" });
    expect(within(fixes).getByText("2 servers need a sign-in")).toBeInTheDocument();
    expect(within(fixes).getByText("issue-tracker, design-files")).toBeInTheDocument();
    expect(within(fixes).getByText("Skills")).toBeInTheDocument();
    await user.click(within(fixes).getByRole("button", { name: "Open Logins" }));
    expect(screen.getByRole("tab", { name: "Logins" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("runs the fix the CLI names after a preview, then runs doctor again", async () => {
    const { user } = await openHealth();
    const before = bridge.count("doctor");
    await user.click(
      within(screen.getByRole("region", { name: "Fixes" })).getByRole("button", {
        name: "Run skills sync",
      }),
    );
    const review = await screen.findByRole("dialog", { name: "Run skills sync?" });
    await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain("skills sync --dry-run");
    expect(bridge.ran()).not.toContain("skills sync");
    await user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByRole("dialog", { name: "Run skills sync" });
    expect(bridge.ran()).toContain("skills sync");
    await waitFor(() => expect(bridge.count("doctor")).toBeGreaterThan(before));
  });

  it("only copies a fix the registry does not know, and never runs it", async () => {
    const odd = clone(serversWorld.doctor);
    odd.checks[6].detail = "something is off; run toolportctl frobnicate";
    bridge.set("doctor", odd);
    await openHealth();
    const fixes = screen.getByRole("region", { name: "Fixes" });
    expect(within(fixes).getByText("toolportctl frobnicate")).toBeInTheDocument();
    expect(
      within(fixes).getByRole("button", { name: "Copy command" }),
    ).toBeInTheDocument();
    expect(
      within(fixes).queryByRole("button", { name: /^Run frobnicate/ }),
    ).not.toBeInTheDocument();
  });

  it("sends a doctor line about direct entries or the active profile to the tab that fixes it", async () => {
    const world = clone(serversWorld.doctor);
    world.checks[5] = {
      name: "directEntries",
      status: "warn",
      detail: "2 direct launcher entries are out of date",
    };
    bridge.set("doctor", world);
    const { user } = await openHealth();
    await user.click(
      within(screen.getByRole("region", { name: "Fixes" })).getByRole("button", {
        name: "Open Clients",
      }),
    );
    expect(screen.getByRole("tab", { name: /^Clients/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("says nothing needs fixing when doctor is clean and every login works", async () => {
    const signedIn = clone(serversWorld.status);
    signedIn.auth.servers = signedIn.auth.servers.filter((row) => row.state === "ok");
    bridge.set("status", signedIn);
    bridge.set("doctor", healthyDoctor());
    await openHealth();
    expect(screen.getByText("Nothing to fix.")).toBeInTheDocument();
  });

  it("keeps the checks of a doctor that exits with a failure", async () => {
    const failing = clone(serversWorld.doctor);
    failing.healthy = false;
    failing.checks[4] = {
      name: "gatewayBinary",
      status: "fail",
      detail: "no gateway binary at /fixture/bin",
    };
    bridge.set("doctor", ctlFailure("unhealthy", "1 check failed", failing));
    const { checks } = await openHealth();
    const row = within(checks).getAllByRole("listitem")[4];
    expect(row).toHaveTextContent("Gateway binary");
    expect(row).toHaveTextContent("Failed");
    expect(row).toHaveTextContent("no gateway binary at /fixture/bin");
    expect(screen.queryByText("Doctor could not run")).not.toBeInTheDocument();
  });

  it("shows why doctor could not run, with Retry", async () => {
    const good = bridge.get("doctor");
    bridge.set("doctor", ctlFailure("crashed", "doctor crashed"));
    const { user } = renderScreen();
    await user.click(tab("Health"));
    const alert = await screen.findByText("Doctor could not run");
    expect(alert.closest("[role=alert]")).toHaveTextContent("doctor crashed");
    bridge.set("doctor", good);
    await user.click(
      within(alert.closest("[role=alert]") as HTMLElement).getByRole("button", {
        name: /Retry/,
      }),
    );
    expect(
      await screen.findByRole("list", { name: "Doctor checks" }),
    ).toBeInTheDocument();
  });

  it("runs doctor again from the header and from the checks", async () => {
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Needs attention" });
    await user.click(screen.getByRole("button", { name: "Run doctor" }));
    await screen.findByRole("list", { name: "Doctor checks" });
    expect(screen.getByRole("tab", { name: "Health" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const first = bridge.count("doctor");
    expect(first).toBeGreaterThan(0);
    const checks = screen.getByRole("region", { name: "Checks" });
    await user.click(within(checks).getByRole("button", { name: "Run doctor" }));
    await waitFor(() => expect(bridge.count("doctor")).toBeGreaterThan(first));
  });

  it("shows the CLI's error when the status cannot be read", async () => {
    bridge.set(
      "status",
      ctlFailure("registry_unreadable", "the registry could not be parsed"),
    );
    const { user } = renderScreen();
    await user.click(tab("Health"));
    const alerts = await screen.findAllByRole("alert");
    expect(
      alerts.some((alert) =>
        alert.textContent?.includes("the registry could not be parsed"),
      ),
    ).toBe(true);
  });
});
