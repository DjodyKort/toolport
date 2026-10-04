import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { button, confirm, group, openSystem, pageText } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";
import { COUNCIL_KEY } from "./world";

/** The Council tab walked the way a person uses it: install, put the key in the vault, check
 * the doctor, uninstall. Each test is named by the parity action it proves
 * (`src/plus/gui-parity.json`) and asserts that the next read changed. */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter(
      (line) =>
        !/^(commands|sync|update|council|mcp|import|secret set council)( |$)/.test(line),
    );
  expect(stray, "the screen only runs its own command groups").toEqual([]);
  expect(bridge.ran().join("\n")).not.toContain("CANARY");
});

const CANARY = "CANARY-council-4b9a";
const checks = () =>
  within(screen.getByRole("list", { name: "Council checks" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);

describe("Council tab, end to end", () => {
  it("system.council-doctor, system.council-tools: a council that is not installed fails its checks in view and still lists its tools", async () => {
    await openSystem(bridge, "Council");
    expect(await screen.findByText("Not installed")).toBeVisible();
    await screen.findByRole("list", { name: "Council checks" });
    expect(checks().join("|")).toMatch(
      /FixRegistry entryrun `toolportctl council install`/,
    );
    expect(checks().join("|")).toMatch(/OKLauncher on the pathuvx/);
    expect(await screen.findByRole("list", { name: "Council tools" })).toBeVisible();
    expect(bridge.count("council doctor")).toBe(1);
    expect(bridge.count("council tools")).toBe(1);
  });

  it("system.council-install: the install is confirmed, then the key goes to the vault on stdin and the doctor turns green", async () => {
    const user = await openSystem(bridge, "Council");
    await user.click(await screen.findByRole("button", { name: "Install…" }));
    const box = await screen.findByRole("dialog", { name: "Install the council?" });
    expect(within(box).getByText(/This command has no preview/)).toBeVisible();
    expect(bridge.world!.state.council.installed).toBe(false);
    await confirm(user, "Install", { done: "Council installed" });
    expect(await screen.findByText("Installed")).toBeVisible();
    await waitFor(() => expect(checks().join("|")).toMatch(/OKRegistry entry/));
    expect(checks().join("|")).toMatch(/FixAPI key in the vault/);

    await user.click(button("Set key"));
    const dialog = await screen.findByRole("dialog", { name: `Set ${COUNCIL_KEY}` });
    const field = within(dialog).getByLabelText("New value");
    expect(field).toHaveAttribute("type", "password");
    await user.type(field, CANARY);
    await user.click(within(dialog).getByRole("button", { name: "Save to vault" }));
    expect(
      await within(dialog).findByText(`${COUNCIL_KEY} is stored for Council.`),
    ).toBeVisible();
    expect(bridge.stdin(`secret set council ${COUNCIL_KEY}`)).toEqual([CANARY]);
    expect(pageText()).not.toContain(CANARY);
    await user.click(within(dialog).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(checks().join("|")).toMatch(/OKAPI key in the vault/));
    expect(await screen.findByRole("button", { name: "Replace key" })).toBeVisible();
  });

  it("system.council-uninstall: Escape cancels and returns focus, the typed uninstall deletes the key on request", async () => {
    bridge.world!.state.council = { installed: true, keyStored: true };
    const user = await openSystem(bridge, "Council");
    const opener = await screen.findByRole("button", { name: "Uninstall…" });
    await user.click(
      screen.getByLabelText("Also delete the stored API key when uninstalling"),
    );
    await user.click(opener);
    const box = await screen.findByRole("dialog", { name: "Uninstall the council?" });
    expect(within(box).getByText("A deleted key cannot be restored")).toBeVisible();
    expect(within(box).getByRole("button", { name: "Uninstall" })).toBeDisabled();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();
    expect(bridge.count("council uninstall --purge-key")).toBe(0);

    await user.click(opener);
    await confirm(user, "Uninstall", {
      typed: "council uninstall",
      done: "Council uninstalled",
    });
    expect(bridge.world!.state.council).toEqual({ installed: false, keyStored: false });
    expect(await screen.findByText("Not installed")).toBeVisible();
    expect(group("Doctor").getByText(/Registry entry/)).toBeVisible();
  });
});
