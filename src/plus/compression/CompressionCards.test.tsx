import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { CompressionTab } from "./CompressionTab";
import { goldenData, goldenFailureOf, ledgerData } from "./fixtures";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const failureOf = (stem: string) => {
  const { code, message, data } = goldenFailureOf(stem);
  return failure(code, message, data);
};

const open = async (name: string | RegExp) => {
  const user = userEvent.setup();
  render(<CompressionTab />);
  await user.click(await screen.findByRole("button", { name }));
  return user;
};

describe("Compression tab: enable, disable, sync", () => {
  it("enables with the form's options, previews with --dry-run and applies on confirm", async () => {
    bridge.set(
      "compression enable --provider rtk-only --mode token --port 8800 --dry-run",
      goldenData("compression-enable.preview"),
    );
    bridge.set(
      "compression enable --provider rtk-only --mode token --port 8800",
      goldenData("compression-enable.apply"),
    );
    const user = await open("Enable…");
    const form = await screen.findByRole("dialog", { name: "Enable compression" });
    await user.selectOptions(within(form).getByLabelText("Mode"), "token");
    await user.type(within(form).getByLabelText("Port"), "8800");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const review = await screen.findByRole("dialog", { name: "Enable rtk-only?" });
    expect(
      within(review).getByText("would save config (provider=rtk-only)"),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain(
      "compression enable --provider rtk-only --mode token --port 8800",
    );
    await user.click(within(review).getByRole("button", { name: "Enable" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain(
        "compression enable --provider rtk-only --mode token --port 8800",
      ),
    );
  });

  it("refuses a port that is not a number", async () => {
    const user = await open("Enable…");
    const form = await screen.findByRole("dialog", { name: "Enable compression" });
    await user.type(within(form).getByLabelText("Port"), "80x");
    expect(within(form).getByRole("button", { name: "Preview" })).toBeDisabled();
    expect(bridge.ran().filter((argv) => argv.startsWith("compression enable"))).toEqual(
      [],
    );
  });

  it("disables with a typed confirmation and --teardown as the checkbox says", async () => {
    bridge.set(
      "compression disable --teardown --dry-run",
      goldenData("compression-disable.preview"),
    );
    bridge.set("compression disable --teardown", goldenData("compression-disable.apply"));
    const user = await open("Disable…");
    const form = await screen.findByRole("dialog", { name: "Disable compression" });
    await user.click(
      within(form).getByRole("checkbox", { name: /tear the engine down/i }),
    );
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const review = await screen.findByRole("dialog", { name: "Disable compression?" });
    expect(
      within(review).getByText("would run `headroom unwrap claude`"),
    ).toBeInTheDocument();
    const confirm = within(review).getByRole("button", { name: "Disable" });
    expect(confirm).toBeDisabled();
    await user.type(within(review).getByRole("textbox"), "disable");
    expect(confirm).toBeEnabled();
    expect(bridge.ran()).not.toContain("compression disable --teardown");
    await user.click(confirm);
    await waitFor(() => expect(bridge.ran()).toContain("compression disable --teardown"));
  });

  it("syncs: preview, then apply", async () => {
    bridge.set("compression sync --dry-run", goldenData("compression-sync.preview"));
    bridge.set("compression sync", goldenData("compression-sync.apply"));
    const user = await open("Sync…");
    await user.click(
      within(await screen.findByRole("dialog", { name: "Sync the policy" })).getByRole(
        "button",
        { name: "Preview" },
      ),
    );
    const review = await screen.findByRole("dialog", {
      name: "Sync the compression policy?",
    });
    await user.click(within(review).getByRole("button", { name: "Sync" }));
    await waitFor(() => expect(bridge.ran()).toContain("compression sync"));
  });
});

describe("Compression tab: engine pin", () => {
  it("shows the pin, the requirement and that nothing is installed", async () => {
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Engine pin" });
    expect(
      await within(card).findByText("headroom-ai[proxy,code,ml]==0.29.0"),
    ).toBeInTheDocument();
    expect(within(card).getByText("Not installed")).toBeInTheDocument();
  });

  it("installs with a network notice after a dry run", async () => {
    const pin = goldenData("compression-pin");
    bridge.set("compression pin --install --dry-run", {
      ...pin,
      dryRun: true,
      install: { requirement: pin.requirement, dryRun: true },
      restartProxies: false,
    });
    bridge.set("compression pin --install", {
      ...pin,
      install: { requirement: pin.requirement, dryRun: false, detail: "none → 0.29.0" },
      restartProxies: true,
    });
    const user = await open("Install");
    const review = await screen.findByRole("dialog", {
      name: /Install the pinned engine/,
    });
    expect(within(review).getByText(/downloads the engine package/)).toBeInTheDocument();
    expect(
      within(review).getByText("Install headroom-ai[proxy,code,ml]==0.29.0"),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("compression pin --install");
    await user.click(within(review).getByRole("button", { name: "Install" }));
    expect(
      await screen.findByText(/Installed headroom-ai.*none → 0\.29\.0/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Restart the proxy to run the pinned build/),
    ).toBeInTheDocument();
  });

  it("sets a pin from a validated version", async () => {
    const pin = goldenData("compression-pin");
    bridge.set("compression pin 0.30.0 --dry-run", {
      ...pin,
      set: true,
      pin: "0.30.0",
      dryRun: true,
    });
    const user = await open("Set pin…");
    const form = await screen.findByRole("dialog", { name: "Set the engine pin" });
    await user.type(within(form).getByLabelText("Version"), "latest");
    expect(within(form).getByRole("button", { name: "Preview" })).toBeDisabled();
    await user.clear(within(form).getByLabelText("Version"));
    await user.type(within(form).getByLabelText("Version"), "0.30.0");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const review = await screen.findByRole("dialog", {
      name: "Pin the engine to 0.30.0?",
    });
    expect(
      within(review).getByText(/Pin the engine to 0\.30\.0 \(requirement/),
    ).toBeInTheDocument();
  });

  it("refreshes the preset knobs from the pin and from the presets list", async () => {
    const refresh = {
      version: "0.29.0",
      presets: [
        {
          name: "agent",
          changed: true,
          added: [{ knob: "HEADROOM_MAX_ITEMS", value: "50" }],
          removed: [{ knob: "HEADROOM_OLD", was: "1" }],
          moved: [{ knob: "HEADROOM_WINDOW", from: "10", to: "12" }],
          kept: ["HEADROOM_ACCURACY_GUARD"],
        },
      ],
    };
    bridge.set("compression presets --refresh --dry-run", {
      ...goldenData("compression-presets"),
      refresh,
    });
    const user = await open("Refresh presets");
    const review = await screen.findByRole("dialog", {
      name: "Refresh the preset knobs?",
    });
    expect(
      within(review).getByText("Re-snapshot preset agent from 0.29.0"),
    ).toBeInTheDocument();
    expect(within(review).getByText("+ HEADROOM_MAX_ITEMS=50")).toBeInTheDocument();
    expect(within(review).getByText("- HEADROOM_OLD (was 1)")).toBeInTheDocument();
    expect(within(review).getByText("~ HEADROOM_WINDOW: 10 → 12")).toBeInTheDocument();
    expect(within(review).getByText(/keeps 1 declared knob/)).toBeInTheDocument();
    await user.click(within(review).getByRole("button", { name: "Cancel" }));
    expect(bridge.ran()).not.toContain("compression presets --refresh");
  });

  it("updates in two steps: the preview has no --accept, the apply has", async () => {
    bridge.set(
      "compression update --to 0.30.0",
      goldenData("compression-update.preview"),
    );
    bridge.set("compression update --to 0.30.0 --accept", {
      ...goldenData("compression-update.preview"),
      accepted: true,
    });
    const user = await open("Update…");
    const form = await screen.findByRole("dialog", { name: "Update the engine" });
    await user.click(within(form).getByRole("radio", { name: /specific version/ }));
    await user.type(within(form).getByLabelText("Version"), "0.30.0");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const review = await screen.findByRole("dialog", {
      name: "Update the engine to 0.30.0?",
    });
    expect(within(review).getByText("Move the pin 0.29.0 → 0.30.0")).toBeInTheDocument();
    expect(
      within(review).getByText(/unverified against the recorded contract/),
    ).toBeInTheDocument();
    expect(within(review).getByText(/downloads the engine package/)).toBeInTheDocument();
    expect(bridge.ran()).toContain("compression update --to 0.30.0");
    expect(bridge.ran()).not.toContain("compression update --to 0.30.0 --accept");
    await user.click(within(review).getByRole("button", { name: "Accept update" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("compression update --to 0.30.0 --accept"),
    );
  });

  it("updates to the latest build with --latest", async () => {
    bridge.set("compression update --latest", goldenData("compression-update.preview"));
    const user = await open("Update…");
    const form = await screen.findByRole("dialog", { name: "Update the engine" });
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(
      await screen.findByRole("dialog", {
        name: "Update the engine to the latest build?",
      }),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("compression update --latest");
  });

  it("shows an error with Retry when the pin cannot be read", async () => {
    const user = userEvent.setup();
    bridge.set(
      "compression pin",
      failure("config_invalid", "compression.json is not valid JSON"),
    );
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Engine pin" });
    expect(await within(card).findByRole("alert")).toHaveTextContent(
      "compression.json is not valid JSON",
    );
    bridge.set("compression pin", goldenData("compression-pin"));
    await user.click(within(card).getByRole("button", { name: /Retry/ }));
    expect(await within(card).findByText("Not installed")).toBeInTheDocument();
  });
});

describe("Compression tab: seal and proxy", () => {
  it("previews the declarable knobs with --dry-run and seals with --apply", async () => {
    bridge.set("compression seal --dry-run", goldenData("compression-seal.preview"));
    bridge.set("compression seal --apply", goldenData("compression-seal.apply"));
    const user = await open("Seal…");
    const review = await screen.findByRole("dialog", {
      name: "Seal the live proxy posture?",
    });
    expect(within(review).getByText("Declare HEADROOM_MAX_ITEMS=50")).toBeInTheDocument();
    expect(
      within(review).getByText("HEADROOM_PROTECT_RECENT stays unset"),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("compression seal --apply");
    await user.click(within(review).getByRole("button", { name: "Seal" }));
    expect(
      await screen.findByText(/Sealed 2 knob\(s\) of preset interactive/),
    ).toBeInTheDocument();
  });

  it("says to start the proxy first when there is none to seal", async () => {
    bridge.set("compression seal --dry-run", failureOf("compression-seal.no-proxy"));
    await open("Seal…");
    expect(await screen.findByText(/no proxy on :49213/)).toBeInTheDocument();
    expect(
      screen.getByText("Start the proxy first, then seal what it runs."),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("compression seal --apply");
  });

  it("starts the proxy after a confirmation, with no preview, and explains a failure", async () => {
    bridge.set("compression proxy up", failureOf("compression-proxy-up.no-engine"));
    const user = await open("Start");
    const confirm = await screen.findByRole("dialog", {
      name: "Start the compression proxy?",
    });
    expect(within(confirm).getByText(/no preview/)).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("compression proxy up");
    await user.click(within(confirm).getByRole("button", { name: "Start" }));
    expect(await screen.findByText("headroom not on PATH")).toBeInTheDocument();
    expect(screen.getByText(/engine \(headroom\) is not installed/)).toBeInTheDocument();
  });

  it("stops the proxy and reports that none is running", async () => {
    bridge.set("compression proxy down", failureOf("compression-proxy-down.no-proxy"));
    const user = await open("Stop");
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Stop the compression proxy?" }),
      ).getByRole("button", { name: "Stop" }),
    );
    expect(await screen.findByText("no proxy listening on :49213")).toBeInTheDocument();
  });

  it("restarts the proxy", async () => {
    bridge.set("compression proxy restart", { restarted: true });
    const user = await open("Restart");
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Restart the compression proxy?" }),
      ).getByRole("button", { name: "Restart" }),
    );
    await waitFor(() => expect(bridge.ran()).toContain("compression proxy restart"));
  });
});

describe("Compression tab: health", () => {
  it("lists the doctor checks and re-runs them on Run doctor", async () => {
    const user = userEvent.setup();
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Health checks" });
    expect(
      await within(card).findByText("not used by provider none"),
    ).toBeInTheDocument();
    expect(within(card).getAllByText("ok")).toHaveLength(3);
    await user.click(screen.getByRole("button", { name: "Run doctor" }));
    await waitFor(() => expect(bridge.count("compression doctor")).toBe(2));
  });

  it("shows the failed checks of a doctor that exits 1", async () => {
    const doctor = goldenData("compression-doctor");
    bridge.set(
      "compression doctor",
      failure("unhealthy", "one or more checks failed", {
        ...doctor,
        healthy: false,
        checks: [
          { name: "engine", ok: false, detail: "headroom not found on PATH" },
          ...doctor.checks.slice(1),
        ],
      }),
    );
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Health checks" });
    expect(
      await within(card).findByText("headroom not found on PATH"),
    ).toBeInTheDocument();
    expect(within(card).getByText("failed")).toBeInTheDocument();
    expect(within(card).queryByRole("alert")).toBeNull();
  });

  it("says the health checks are offline when toolportctl cannot be reached", async () => {
    bridge.set("compression doctor", () => {
      throw new Error("toolportctl could not be started");
    });
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Health checks" });
    expect(await within(card).findByRole("alert")).toHaveTextContent(
      "toolportctl could not be started",
    );
  });

  it("verifies local transcripts and says it makes no model requests", async () => {
    bridge.set(
      "compression verify --min-turns 1",
      goldenData("compression-verify.measured"),
    );
    const user = await open("Verify…");
    const dialog = await screen.findByRole("dialog", { name: "Verify compression" });
    expect(
      within(dialog).getByText(/Reads local transcripts, makes no model requests/),
    ).toBeInTheDocument();
    expect(bridge.ran().filter((argv) => argv.startsWith("compression verify"))).toEqual(
      [],
    );
    await user.type(within(dialog).getByLabelText("Minimum turns"), "1");
    await user.click(within(dialog).getByRole("button", { name: "Run verify" }));
    const table = await within(dialog).findByRole("table", {
      name: "Cache behaviour by launch",
    });
    const row = within(table).getByRole("row", { name: /Unattributed/ });
    expect(within(row).getByText("42,000")).toBeInTheDocument();
    expect(within(row).getByText("0.93")).toBeInTheDocument();
    expect(within(dialog).getByText(/1 transcript read from/)).toBeInTheDocument();
  });

  it("shows the checks and a hint when there are no transcripts to measure", async () => {
    bridge.set(
      "compression verify --transcripts /work/none",
      failureOf("compression-verify.no-transcripts"),
    );
    const user = await open("Verify…");
    const dialog = await screen.findByRole("dialog", { name: "Verify compression" });
    await user.type(within(dialog).getByLabelText("Transcripts folder"), "/work/none");
    await user.click(within(dialog).getByRole("button", { name: "Run verify" }));
    expect(
      await within(dialog).findByText(/No transcripts to measure under/),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText("not required for this provider"),
    ).toBeInTheDocument();
  });
});

describe("Compression tab: ledger", () => {
  it("shows the empty state before any launch", async () => {
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Savings ledger" });
    expect(await within(card).findByText("No launches recorded yet")).toBeInTheDocument();
  });

  it("charts before and after per provider and repeats the numbers in a table", async () => {
    bridge.set("compression ledger summary", ledgerData());
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Savings ledger" });
    const chart = await within(card).findByRole("img");
    expect(chart).toHaveAccessibleName(/headroom: 20,000 tokens before, 8,000 after/);
    expect(chart).toHaveAccessibleName(/rtk-only: 1,000 tokens before, 1,400 after/);
    const row = within(
      within(card).getByRole("table", { name: "Savings by provider" }),
    ).getByRole("row", { name: /headroom/ });
    expect(within(row).getByText("12,000")).toBeInTheDocument();
    expect(within(row).getByText("60.0%")).toBeInTheDocument();
    expect(within(card).getByText("6,600")).toBeInTheDocument();
  });

  it("records a savings entry through a direct confirmation, then reads the ledger again", async () => {
    bridge.set(
      "compression ledger record --provider rtk-only --before 1000 --after 400 --source contract --session s1",
      goldenData("compression-ledger-record.apply"),
    );
    const user = await open("Record savings…");
    const form = await screen.findByRole("dialog", { name: "Record savings" });
    expect(within(form).getByRole("button", { name: "Review" })).toBeDisabled();
    await user.type(within(form).getByLabelText("Tokens before"), "1000");
    await user.type(within(form).getByLabelText("Tokens after"), "400");
    await user.type(within(form).getByLabelText("Source"), "contract");
    await user.type(within(form).getByLabelText("Session"), "s1");
    bridge.set("compression ledger summary", ledgerData());
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Record a savings entry?",
    });
    await user.click(within(confirm).getByRole("button", { name: "Record" }));
    await waitFor(() => expect(bridge.count("compression ledger summary")).toBe(2));
    expect(bridge.ran().filter((argv) => argv.includes("--dry-run"))).toEqual([]);
  });

  it("shows an error with Retry when the ledger cannot be read", async () => {
    bridge.set(
      "compression ledger summary",
      failure("ledger_read", "the ledger file is unreadable"),
    );
    render(<CompressionTab />);
    const card = await screen.findByRole("region", { name: "Savings ledger" });
    expect(await within(card).findByRole("alert")).toHaveTextContent(
      "the ledger file is unreadable",
    );
    expect(within(card).getByRole("button", { name: /Retry/ })).toBeInTheDocument();
  });
});

describe("Compression tab: run and env", () => {
  const dir = "/work/acme";

  it("always shows the terminal command, copy-only, with Open in Terminal off", async () => {
    render(<CompressionTab />);
    const card = await screen.findByRole("region", {
      name: "Run Claude under this policy",
    });
    expect(within(card).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl compression run -- claude",
    );
    expect(within(card).getByRole("button", { name: "Copy command" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: /Open in Terminal/ })).toBeDisabled();
    expect(bridge.ran().filter((argv) => argv.startsWith("compression run"))).toEqual([]);
  });

  it("plans the launch and prints the env for a folder, and never runs Claude", async () => {
    bridge.set(`compression run --plan --cwd ${dir} claude`, {
      ...goldenData("compression-run.plan"),
      cwd: dir,
      routed: true,
      provider: "headroom",
      ledger: { ...goldenData("compression-run.plan").ledger, routed: true, port: 8787 },
      env: { set: { ANTHROPIC_BASE_URL: "http://127.0.0.1:8787" }, unset: [] },
      warnings: ["the proxy is not running"],
    });
    bridge.set(`compression env --cwd ${dir}`, {
      ...goldenData("compression-env"),
      cwd: dir,
      launch: "routed",
      lines: ["HRCOMPRESS_LAUNCH=routed", "ANTHROPIC_BASE_URL=http://127.0.0.1:8787"],
    });
    const user = userEvent.setup();
    render(<CompressionTab />);
    const card = await screen.findByRole("region", {
      name: "Run Claude under this policy",
    });
    await user.type(within(card).getByLabelText("Folder"), dir);
    expect(within(card).getByLabelText("Command line")).toHaveTextContent(
      `toolportctl compression run --cwd ${dir} -- claude`,
    );
    await user.click(within(card).getByRole("button", { name: "Preview this folder" }));
    const plan = await within(card).findByLabelText("Launch plan");
    expect(within(plan).getByText("routed")).toBeInTheDocument();
    expect(
      within(plan).getByText("ANTHROPIC_BASE_URL=http://127.0.0.1:8787"),
    ).toBeInTheDocument();
    expect(within(plan).getByText("the proxy is not running")).toBeInTheDocument();
    expect(await within(card).findByLabelText("Env lines")).toHaveTextContent(
      "HRCOMPRESS_LAUNCH=routed",
    );
    expect(bridge.ran()).not.toContain(`compression run --cwd ${dir} -- claude`);
  });

  it("shows the CLI's error when the plan cannot be made", async () => {
    bridge.set(
      `compression run --plan --cwd ${dir} claude`,
      failure("usage", "no such folder"),
    );
    bridge.set(`compression env --cwd ${dir}`, goldenData("compression-env"));
    const user = userEvent.setup();
    render(<CompressionTab />);
    const card = await screen.findByRole("region", {
      name: "Run Claude under this policy",
    });
    await user.type(within(card).getByLabelText("Folder"), dir);
    await user.click(within(card).getByRole("button", { name: "Preview this folder" }));
    expect(await within(card).findByRole("alert")).toHaveTextContent("no such folder");
  });

  it("leaks no credential from a launch env into the page or the copy text", async () => {
    const canary = "canary-TOOLPORT_SECRET_KEY-9f3a1c";
    bridge.set(`compression run --plan --cwd ${dir} claude`, {
      ...goldenData("compression-run.plan"),
      env: { set: { TOOLPORT_SECRET_KEY: canary, ANTHROPIC_API_KEY: canary }, unset: [] },
    });
    bridge.set(`compression env --cwd ${dir}`, {
      ...goldenData("compression-env"),
      lines: [`TOOLPORT_SECRET_KEY=${canary}`, "HRCOMPRESS_LAUNCH=plain"],
    });
    const user = userEvent.setup();
    render(<CompressionTab />);
    const card = await screen.findByRole("region", {
      name: "Run Claude under this policy",
    });
    await user.type(within(card).getByLabelText("Folder"), dir);
    await user.click(within(card).getByRole("button", { name: "Preview this folder" }));
    await within(card).findByLabelText("Env lines");
    await user.click(within(card).getByRole("button", { name: "Copy env" }));
    expect(document.body.textContent).not.toContain(canary);
    expect(await navigator.clipboard.readText()).toBe(
      "TOOLPORT_SECRET_KEY=(hidden)\nHRCOMPRESS_LAUNCH=plain",
    );
    expect(
      within(card).getByText(/TOOLPORT_SECRET_KEY=\(hidden\)/, { selector: "code" }),
    ).toBeInTheDocument();
  });
});
