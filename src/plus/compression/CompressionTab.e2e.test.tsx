import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import { confirm, openCompression, section, stat } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";
import type { WorldState } from "./world";

/** The Compression tab walked the way a person uses it, against a world that changes: a
 * provider switch changes the status strip and the health checks, an install changes the
 * engine and the pin, a recorded entry grows the ledger. Each test is named by the parity
 * action it proves (`src/plus/gui-parity.json`) and asserts that the next read changed. */
let bridge: Bridge;
const start = (world: boolean | Partial<WorldState> = true) => {
  bridge = createBridge({ world });
  wire({ invoke, listen }, bridge);
};
beforeEach(() => start());

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge.ran().filter((line) => !/^(commands|compression)( |$)/.test(line));
  expect(stray, "the tab only runs its own commands").toEqual([]);
  expect(
    bridge.ran().some((line) => /--home|secret|--reveal|stdin|token/.test(line)),
  ).toBe(false);
});

const button = (name: string | RegExp) => screen.getByRole("button", { name });
const checks = () =>
  within(screen.getByRole("region", { name: "Health checks" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);

describe("Compression tab, end to end: policy", () => {
  it("compression.status: opens on today's state and says what is not set up", async () => {
    await openCompression();
    expect(stat("Provider")).toHaveTextContent("rtk-only");
    expect(stat("Provider")).toHaveTextContent("hook runtime");
    expect(stat("Preset")).toHaveTextContent("interactive");
    expect(stat("Engine")).toHaveTextContent("not used by this provider");
    expect(stat("Pin")).toHaveTextContent("0.29.0");
    expect(section("Engine pin").getByText("Not installed")).toBeVisible();
    expect(
      await section("Savings ledger").findByText("No launches recorded yet"),
    ).toBeVisible();
    expect(await section("Health checks").findByText(/rtk binary found/)).toBeVisible();
    expect(bridge.count("compression status")).toBe(1);
  });

  it("compression.set-provider: a preview changes nothing, the apply changes the strip and the health", async () => {
    const user = await openCompression();
    await user.click(button("Switch to headroom"));
    const box = await screen.findByRole("dialog", {
      name: "Switch the provider to headroom?",
    });
    expect(
      within(box).getByText(/would save config \(provider=headroom\)/),
    ).toBeVisible();
    expect(within(box).getAllByText(/headroom is not on PATH/)).toHaveLength(2);
    expect(stat("Provider")).toHaveTextContent("rtk-only");
    expect(bridge.ran()).toContain("compression set-provider headroom --dry-run");
    expect(bridge.count("compression set-provider headroom")).toBe(0);
    await confirm(user, "Switch provider", { done: /^Provider headroom \(proxy\)/ });
    await waitFor(() => expect(stat("Provider")).toHaveTextContent("headroom"));
    expect(stat("Provider")).toHaveTextContent("proxy runtime");
    expect(stat("Engine")).toHaveTextContent("Not installed");
    await waitFor(() =>
      expect(checks().join("|")).toMatch(/engine binaryfailedheadroom not on PATH/),
    );
    expect(section("Provider").getByText("active").closest("div")).toHaveTextContent(
      "headroom",
    );
    expect(screen.queryByRole("button", { name: "Switch to headroom" })).toBeNull();
    expect(screen.getByRole("button", { name: "Switch to rtk-only" })).toBeVisible();
  });

  it("compression.use: another preset becomes the active one", async () => {
    const user = await openCompression();
    const presets = section("Presets");
    await user.click(await presets.findByRole("button", { name: "Use agent" }));
    await confirm(user, "Use preset", { done: /preset agent: token mode, port 8788/ });
    await waitFor(() => expect(stat("Preset")).toHaveTextContent("agent"));
    expect(stat("Preset")).toHaveTextContent("token mode · port 8788");
    expect(presets.getByRole("button", { name: "Use interactive" })).toBeVisible();
    expect(presets.queryByRole("button", { name: "Use agent" })).toBeNull();
  });

  it("compression.enable, compression.disable: enable headroom, then turn it off with a typed phrase", async () => {
    const user = await openCompression();
    await user.click(button("Enable…"));
    const form = await screen.findByRole("dialog", { name: "Enable compression" });
    await user.selectOptions(within(form).getByLabelText("Provider"), "headroom");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await confirm(user, "Enable", { done: /^Provider headroom \(proxy\)/ });
    await waitFor(() => expect(stat("Provider")).toHaveTextContent("headroom"));
    expect(bridge.ran()).toContain("compression enable --provider headroom --dry-run");

    await user.click(button("Disable…"));
    const options = await screen.findByRole("dialog", { name: "Disable compression" });
    await user.click(within(options).getByRole("button", { name: "Preview" }));
    await confirm(user, "Disable", { typed: "disable", done: /^Provider none \(none\)/ });
    await waitFor(() => expect(stat("Provider")).toHaveTextContent("none"));
    expect(stat("Engine")).toHaveTextContent("Off");
  });

  it("compression.sync: re-applying the policy previews first and leaves the provider alone", async () => {
    const user = await openCompression();
    await user.click(button("Sync…"));
    const form = await screen.findByRole("dialog", { name: "Sync the policy" });
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await confirm(user, "Sync", { done: /^Provider rtk-only \(hook\)/ });
    expect(bridge.ran()).toEqual(
      expect.arrayContaining(["compression sync --dry-run", "compression sync"]),
    );
    expect(stat("Provider")).toHaveTextContent("rtk-only");
  });
});

describe("Compression tab, end to end: engine", () => {
  const switchTo = async (
    user: Awaited<ReturnType<typeof openCompression>>,
    provider: string,
  ) => {
    await user.click(button(`Switch to ${provider}`));
    await confirm(user, "Switch provider");
    await waitFor(() => expect(stat("Provider")).toHaveTextContent(provider));
  };

  it("compression.pin: the pinned engine is installed and the Engine tile and the pin card follow", async () => {
    const user = await openCompression();
    await switchTo(user, "headroom");
    const pin = section("Engine pin");
    expect(pin.getByText("Not installed")).toBeVisible();
    expect(stat("Engine")).toHaveTextContent("Not installed");
    await user.click(pin.getByRole("button", { name: "Install" }));
    const box = await screen.findByRole("dialog", {
      name: "Install the pinned engine 0.29.0?",
    });
    expect(
      within(box).getByText(/downloads the engine package from the network/),
    ).toBeVisible();
    expect(bridge.count("compression pin --install")).toBe(0);
    await confirm(user, "Install", {
      done: /Installed headroom-ai\[proxy,code,ml\]==0\.29\.0/,
    });
    await waitFor(() => expect(stat("Engine")).toHaveTextContent("Healthy"));
    expect(stat("Engine")).toHaveTextContent("0.29.0 (pinned)");
    expect(pin.getByText("matches the pin")).toBeVisible();
    expect(pin.queryByText("Not installed")).toBeNull();
  });

  it("compression.pin: a new pin is set from a version and shows as drift against the installed build", async () => {
    const user = await openCompression();
    await switchTo(user, "headroom");
    await user.click(section("Engine pin").getByRole("button", { name: "Install" }));
    await confirm(user, "Install");
    await user.click(section("Engine pin").getByRole("button", { name: "Set pin…" }));
    const form = await screen.findByRole("dialog", { name: "Set the engine pin" });
    await user.type(within(form).getByLabelText("Version"), "0.31.0");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await confirm(user, "Set pin", { done: "Engine pin 0.31.0" });
    await waitFor(() => expect(stat("Pin")).toHaveTextContent("0.31.0"));
    expect(stat("Engine")).toHaveTextContent("Drift");
    expect(section("Engine pin").getByText("differs from the pin")).toBeVisible();
  });

  it("compression.update: the update is previewed as a move, accepted, and moves the pin and the build", async () => {
    const user = await openCompression();
    await switchTo(user, "headroom");
    await user.click(section("Engine pin").getByRole("button", { name: "Update…" }));
    const form = await screen.findByRole("dialog", { name: "Update the engine" });
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", {
      name: "Update the engine to the latest build?",
    });
    expect(within(box).getByText(/Move the pin 0\.29\.0 → 0\.30\.0/)).toBeVisible();
    expect(stat("Pin")).toHaveTextContent("0.29.0");
    await confirm(user, "Accept update", { done: "Update the engine pin to 0.30.0" });
    await waitFor(() => expect(stat("Pin")).toHaveTextContent("0.30.0"));
    expect(stat("Engine")).toHaveTextContent("0.30.0 (pinned)");
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "compression update --latest",
        "compression update --latest --accept",
      ]),
    );
  });

  it("compression.presets: refreshing the knobs needs the engine, then snapshots the presets from it", async () => {
    const user = await openCompression();
    await user.click(button("Refresh presets"));
    expect(await screen.findByText(/headroom not on PATH/)).toBeVisible();
    await user.keyboard("{Escape}");
    await switchTo(user, "headroom");
    await user.click(section("Engine pin").getByRole("button", { name: "Install" }));
    await confirm(user, "Install");
    await user.click(button("Refresh presets"));
    await confirm(user, "Refresh", { done: /Re-snapshot preset agent from 0\.29\.0/ });
    const presets = section("Presets");
    await waitFor(() =>
      expect(presets.getAllByText(/snapshot 0\.29\.0/)).toHaveLength(2),
    );
    expect(presets.getAllByText(/1 knobs/)).toHaveLength(2);
  });

  it("compression.proxy, compression.seal: the proxy needs the engine, the seal needs the proxy", async () => {
    start({ provider: "headroom" });
    const user = await openCompression();
    const proxy = within(screen.getByRole("group", { name: "Proxy" }));
    await user.click(proxy.getByRole("button", { name: "Start" }));
    await user.click(await screen.findByRole("button", { name: "Start" }));
    expect(
      await screen.findByText(/The engine \(headroom\) is not installed/),
    ).toBeVisible();
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );

    await user.click(button("Seal…"));
    expect(await screen.findByText(/Start the proxy first, then seal/)).toBeVisible();
    expect(bridge.count("compression seal --apply")).toBe(0);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );

    await user.click(section("Engine pin").getByRole("button", { name: "Install" }));
    await confirm(user, "Install");
    await user.click(proxy.getByRole("button", { name: "Start" }));
    await confirm(user, "Start", { done: /started proxy on :8787/ });
    await waitFor(() =>
      expect(checks().join("|")).toMatch(/engine reachableokproxy ready on :8787/),
    );
    expect(checks().join("|")).toMatch(
      /sealed posturefailedvendor-decided, declarable: HEADROOM_MAX_ITEMS=50/,
    );

    await user.click(button("Seal…"));
    await confirm(user, "Seal", {
      done: "Sealed 2 knob(s) of preset interactive (proxy on port 8787)",
    });
    await waitFor(() =>
      expect(checks().join("|")).toMatch(
        /sealed postureokevery declarable knob is policy/,
      ),
    );

    await user.click(proxy.getByRole("button", { name: "Stop" }));
    await confirm(user, "Stop", { done: /stopped proxy on :8787/ });
    await waitFor(() =>
      expect(checks().join("|")).toMatch(/engine reachablefailedno ready proxy/),
    );
  });
});

describe("Compression tab, end to end: health and ledger", () => {
  it("compression.doctor: the checks follow the policy and Run doctor reads them again", async () => {
    start({ provider: "headroom" });
    const user = await openCompression();
    await waitFor(() => expect(checks().join("|")).toMatch(/engine binaryfailed/));
    expect(bridge.count("compression doctor")).toBe(1);
    await user.click(section("Engine pin").getByRole("button", { name: "Install" }));
    await confirm(user, "Install");
    await waitFor(() =>
      expect(checks().join("|")).toMatch(/engine binaryok0\.29\.0 == pin 0\.29\.0/),
    );
    const before = bridge.count("compression doctor");
    await user.click(button("Run doctor"));
    await waitFor(() => expect(bridge.count("compression doctor")).toBe(before + 1));
  });

  it("compression.verify: measures the transcripts and says so when there are none", async () => {
    const user = await openCompression();
    await user.click(button("Verify…"));
    const box = await screen.findByRole("dialog", { name: "Verify compression" });
    expect(within(box).getByText(/makes no model requests/)).toBeVisible();
    await user.click(within(box).getByRole("button", { name: "Run verify" }));
    const table = await within(box).findByRole("table", {
      name: "Cache behaviour by launch",
    });
    expect(within(table).getByText("Unattributed")).toBeVisible();
    await user.click(within(box).getByRole("button", { name: "Run again" }));
    await user.type(within(box).getByLabelText("Transcripts folder"), "/fixture/none");
    await user.click(within(box).getByRole("button", { name: "Run verify" }));
    expect(
      await within(box).findByText(/No transcripts to measure under \/fixture\/none/),
    ).toBeVisible();
    expect(bridge.ran()).toContain("compression verify --transcripts /fixture/none");
    await user.click(within(box).getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("compression.ledger: the empty ledger fills from a recorded entry and the totals stay true", async () => {
    const user = await openCompression();
    const ledger = section("Savings ledger");
    expect(await ledger.findByText("No launches recorded yet")).toBeVisible();
    const record = async (provider: string, before: string, after: string) => {
      await user.click(button("Record savings…"));
      const form = await screen.findByRole("dialog", { name: "Record savings" });
      await user.selectOptions(within(form).getByLabelText("Provider"), provider);
      await user.type(within(form).getByLabelText("Tokens before"), before);
      await user.type(within(form).getByLabelText("Tokens after"), after);
      await user.click(within(form).getByRole("button", { name: "Review" }));
      await confirm(user, "Record");
    };
    await record("rtk-only", "1000", "400");
    const table = await ledger.findByRole("table", { name: "Savings by provider" });
    expect(within(table).getByText("60.0%")).toBeVisible();
    expect(ledger.getByRole("img")).toHaveAccessibleName(
      /rtk-only: 1,000 tokens before, 400 after/,
    );
    await record("headroom", "20000", "8000");
    await waitFor(() => expect(within(table).getAllByRole("row")).toHaveLength(3));
    expect(ledger.getByText("12,600")).toBeVisible();
    expect(ledger.getByRole("img")).toHaveAccessibleName(
      /headroom: 20,000 tokens before, 8,000 after/,
    );
    expect(bridge.ran()).toContain(
      "compression ledger record --provider headroom --before 20000 --after 8000",
    );
  });
});

describe("Compression tab, end to end: run in a folder", () => {
  const folder = async (user: Awaited<ReturnType<typeof openCompression>>) => {
    await user.type(screen.getByLabelText("Folder"), "/fixture/proj");
    await user.click(button("Preview this folder"));
  };

  it("compression.run, compression.env: a plain policy plans a plain launch and the terminal line is only copied", async () => {
    const user = await openCompression();
    const run = section("Run Claude under this policy");
    expect(run.getByLabelText("Command line")).toHaveTextContent(
      "compression run -- claude",
    );
    expect(run.getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    await folder(user);
    const plan = await run.findByLabelText("Launch plan");
    expect(plan).toHaveTextContent("Plain launch (provider rtk-only)");
    expect(plan).toHaveTextContent("unsets ANTHROPIC_BASE_URL");
    expect(await run.findByLabelText("Env lines")).toHaveTextContent(
      "HRCOMPRESS_LAUNCH=plain",
    );
    expect(run.getByLabelText("Command line")).toHaveTextContent(
      "compression run --cwd /fixture/proj -- claude",
    );
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "compression run --plan --cwd /fixture/proj claude",
        "compression env --cwd /fixture/proj",
      ]),
    );
    expect(
      bridge
        .ran()
        .some((line) => /^compression run( |$)/.test(line) && !line.includes("--plan")),
    ).toBe(false);
  });

  it("compression.run: with the pinned engine installed the plan routes through the proxy", async () => {
    start({ provider: "headroom", installed: "0.29.0", proxy: true });
    const user = await openCompression();
    await folder(user);
    const run = section("Run Claude under this policy");
    expect(await run.findByLabelText("Launch plan")).toHaveTextContent(
      "Through headroom, preset interactive, port 8787",
    );
    expect(await run.findByLabelText("Env lines")).toHaveTextContent(
      'export ANTHROPIC_BASE_URL="http://127.0.0.1:8787"',
    );
  });
});

describe("Compression tab, end to end: keyboard, outage and secrets", () => {
  it("compression.set-provider, compression.disable: every step works with the keyboard alone", async () => {
    const user = await openCompression();
    button("Switch to headroom").focus();
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("dialog", { name: "Switch the provider to headroom?" }),
    ).toBeVisible();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count("compression set-provider headroom")).toBe(0);
    expect(stat("Provider")).toHaveTextContent("rtk-only");

    button("Disable…").focus();
    await user.keyboard(" ");
    const options = await screen.findByRole("dialog", { name: "Disable compression" });
    within(options).getByRole("button", { name: "Preview" }).focus();
    await user.keyboard("{Enter}");
    const box = await screen.findByRole("dialog", { name: "Disable compression?" });
    const confirmButton = within(box).getByRole("button", { name: "Disable" });
    expect(confirmButton).toBeDisabled();
    within(box).getByRole("textbox").focus();
    await user.keyboard("disable");
    expect(confirmButton).toBeEnabled();
    confirmButton.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(stat("Provider")).toHaveTextContent("none"));
  });

  it("compression.status: says so when toolportctl cannot run, and recovers on Retry", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    const user = userEvent.setup();
    render(<PlusViews view="tokens" onSelectView={() => {}} />);
    await user.click(
      within(await screen.findByRole("tablist", { name: "Tokens sections" })).getByRole(
        "tab",
        { name: "Compression" },
      ),
    );
    const failed = (
      await screen.findByText("Couldn't read the compression policy")
    ).closest('[role="alert"]') as HTMLElement;
    expect(failed).toHaveTextContent(/toolportctl could not be started/);
    expect(screen.queryByLabelText("Compression status")).toBeNull();
    down = false;
    await user.click(within(failed).getByRole("button", { name: "Retry" }));
    expect(await screen.findByLabelText("Compression status")).toHaveTextContent(
      "rtk-only",
    );
  });

  it("compression.env: a credential in a launch env is never shown or copied", async () => {
    const canary = "CANARY-secret-5e2d";
    bridge.set("compression env --cwd /fixture/proj", {
      cwd: "/fixture/proj",
      env: {},
      launch: "plain",
      lines: [`export TOOLPORT_SECRET_KEY="${canary}"`, "HRCOMPRESS_LAUNCH=plain"],
      port: null,
      preset: "interactive",
      provider: "rtk-only",
    });
    bridge.set("compression run --plan --cwd /fixture/proj claude", {
      ...(bridge.world!.reply([
        "compression",
        "run",
        "--plan",
        "--cwd",
        "/fixture/proj",
        "claude",
      ]) as object),
      env: { set: { ANTHROPIC_API_KEY: canary }, unset: [] },
    });
    const user = await openCompression();
    await user.type(screen.getByLabelText("Folder"), "/fixture/proj");
    await user.click(button("Preview this folder"));
    expect(await screen.findByLabelText("Env lines")).toHaveTextContent(
      "TOOLPORT_SECRET_KEY=(hidden)",
    );
    expect(await screen.findByText(/ANTHROPIC_API_KEY=\(hidden\)/)).toBeVisible();
    expect(document.body.textContent).not.toContain(canary);
    expect(JSON.stringify(invoke.mock.calls)).not.toContain(canary);
  });
});
