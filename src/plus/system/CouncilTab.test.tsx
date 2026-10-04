import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { CouncilTab } from "./CouncilTab";
import { open, write } from "./harness";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";

const CANARY = "canary-council-key-3";
let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const healthy = () => ({
  checks: golden("council-doctor").checks.map((check: object) => ({
    ...check,
    ok: true,
  })),
});

describe("Council tab: reading", () => {
  it("says the council is not installed and what the doctor found", async () => {
    await open(<CouncilTab />, bridge);
    expect(await screen.findByText("Not installed")).toBeInTheDocument();
    const doctor = screen.getByRole("list", { name: "Council checks" });
    expect(within(doctor).getAllByText("Fix")).toHaveLength(5);
    expect(within(doctor).getByText("Registry entry")).toBeInTheDocument();
    expect(within(doctor).getByText("API key in the vault")).toBeInTheDocument();
    expect(screen.queryByText("one or more checks failed")).toBeNull();
  });

  it("lists the council tools with their tiers and its resources", async () => {
    await open(<CouncilTab />, bridge);
    const tools = await screen.findByRole("list", { name: "Council tools" });
    expect(within(tools).getAllByRole("listitem")).toHaveLength(4);
    expect(within(tools).getByText("council_config_set")).toBeInTheDocument();
    expect(within(tools).getByText("Confirm")).toBeInTheDocument();
    expect(
      within(screen.getByRole("list", { name: "Council resources" })).getByText(
        "council://config",
      ),
    ).toBeInTheDocument();
  });

  it("shows the failures of the doctor and of the tools with Retry", async () => {
    bridge.set("council doctor", failure("council", "the registry is locked"));
    bridge.set("council tools", failure("council", "no tools answer"));
    await open(<CouncilTab />, bridge);
    expect(await screen.findByText("the registry is locked")).toBeInTheDocument();
    expect(await screen.findByText("no tools answer")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(2);
  });

  it("says when the council lists no tools", async () => {
    bridge.set("council tools", { tools: [], resources: [] });
    await open(<CouncilTab />, bridge);
    expect(await screen.findByText("The council lists no tools.")).toBeInTheDocument();
  });
});

describe("Council tab: install, key, uninstall", () => {
  it("installs with a confirmation that says there is no preview, then re-checks", async () => {
    let installed = false;
    bridge.set("council doctor", () =>
      installed ? healthy() : golden("council-doctor"),
    );
    bridge.set("council install", () => {
      installed = true;
      return golden("council-install.apply");
    });
    const { user } = await open(<CouncilTab />, bridge);
    await screen.findByText("Not installed");
    await user.click(screen.getByRole("button", { name: "Install…" }));
    const box = await screen.findByRole("dialog");
    expect(within(box).getByText("Register the council server")).toBeInTheDocument();
    expect(within(box).getByText(/no preview/)).toBeInTheDocument();
    expect(within(box).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl council install",
    );
    expect(within(box).queryByRole("textbox")).toBeNull();
    await user.click(within(box).getByRole("button", { name: "Install" }));
    expect(await screen.findByText("Council installed")).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("council install --api-key-env");
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(
      await screen.findByText(/The council server is in your registry/),
    ).toBeInTheDocument();
  });

  it("stores the API key with secret set on stdin and keeps it out of argv and the DOM", async () => {
    bridge.set("council doctor", healthy());
    bridge.set("secret set council OPENROUTER_API_KEY", { stored: true });
    const { user } = await open(<CouncilTab />, bridge);
    await screen.findByText(/The council server is in your registry/);
    await user.click(screen.getByRole("button", { name: "Replace key" }));
    const box = await screen.findByRole("dialog");
    await user.type(within(box).getByLabelText("New value"), CANARY);
    expect(document.body.innerHTML).not.toContain(CANARY);
    await user.click(within(box).getByRole("button", { name: "Save to vault" }));
    expect(await screen.findByText("Saved to the vault")).toBeInTheDocument();
    expect(bridge.stdin("secret set council OPENROUTER_API_KEY")).toEqual([CANARY]);
    expect(JSON.stringify(bridge.calls.map((call) => call.argv))).not.toContain(CANARY);
    expect(document.body.innerHTML).not.toContain(CANARY);
  });

  it("uninstalls with a typed confirmation and keeps the key unless asked to purge it", async () => {
    bridge.set("council doctor", healthy());
    bridge.set("council uninstall", golden("council-uninstall.apply"));
    const { user } = await open(<CouncilTab />, bridge);
    await screen.findByText(/The council server is in your registry/);
    await write(user, "Uninstall…", "Uninstall", {
      plan: "Remove the council server",
      typed: "council uninstall",
      done: "Council uninstalled",
    });
    expect(bridge.ran()).toContain("council uninstall");
  });

  it("passes --purge-key and warns that a deleted key cannot be restored", async () => {
    bridge.set("council doctor", healthy());
    bridge.set("council uninstall --purge-key", golden("council-uninstall.apply"));
    const { user } = await open(<CouncilTab />, bridge);
    await screen.findByText(/The council server is in your registry/);
    await user.click(screen.getByRole("checkbox", { name: /delete the stored API key/ }));
    await write(user, "Uninstall…", "Uninstall", {
      plan: "Remove the council server",
      typed: "council uninstall",
      done: "Council uninstalled",
    });
    expect(bridge.ran()).toContain("council uninstall --purge-key");
  });

  it("shows a failed install and keeps the tab", async () => {
    bridge.set("council install", failure("council", "the registry is read-only"));
    const { user } = await open(<CouncilTab />, bridge);
    await screen.findByText("Not installed");
    await user.click(screen.getByRole("button", { name: "Install…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Install" }),
    );
    expect(await screen.findByText("the registry is read-only")).toBeInTheDocument();
  });
});
