import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { UsageTab } from "./UsageTab";
import {
  bridgeDown,
  createBridge,
  deferred,
  failure,
  goldenData,
  registryWithTier,
  wire,
  type Bridge,
} from "./testkit";
import { statusOn } from "./world";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

function open() {
  const user = userEvent.setup();
  const view = render(<UsageTab today="2026-10-04" />);
  return { ...view, user };
}

const card = () => screen.findByRole("region", { name: "OpenTelemetry receiver" });
const cardNow = () =>
  screen.getByRole("region", { name: "OpenTelemetry receiver", hidden: true });
const ENABLE = goldenData("obs-otel-enable.preview");
const DISABLE = goldenData("obs-otel-disable.preview");

describe("usage.otel-status: the OTel card shows the receiver and the settings", () => {
  it("shows a receiver that is off, the settings and what Enable touches", async () => {
    open();
    const region = await card();
    expect(await within(region).findByText("Off")).toBeInTheDocument();
    expect(within(region).getByText("http://127.0.0.1:4318")).toBeInTheDocument();
    expect(within(region).getByText("none yet")).toBeInTheDocument();
    expect(within(region).getByText("No telemetry keys")).toBeInTheDocument();
    expect(
      within(region).getByText("/fixture/home/.claude/settings.json"),
    ).toBeInTheDocument();
    expect(
      within(region).getByText(/touches 5 keys in your Claude settings/),
    ).toBeInTheDocument();
    const keys = within(region).getByRole("list", {
      name: "Telemetry keys in the env block of the Claude settings",
    });
    expect(within(keys).getAllByRole("listitem")).toHaveLength(5);
    expect(within(keys).getAllByText("not set")).toHaveLength(5);
    expect(within(region).getByLabelText("Port")).toHaveValue("4318");
    expect(within(region).getByRole("button", { name: "Enable…" })).toBeEnabled();
    expect(within(region).queryByRole("button", { name: "Disable…" })).toBeNull();
  });

  it("shows a listening receiver, its events and the requests the transcripts lack", async () => {
    bridge.set("obs otel status", statusOn);
    open();
    const region = await card();
    expect(await within(region).findByText("Listening")).toBeInTheDocument();
    expect(
      within(region).getByText("17, newest 2026-10-03 16:30 UTC"),
    ).toBeInTheDocument();
    expect(within(region).getByText("Every telemetry key is set")).toBeInTheDocument();
    expect(within(region).getAllByText("set")).toHaveLength(5);
    expect(within(region).getByText("1 of 5 API requests")).toBeInTheDocument();
    expect(within(region).getByText(/cost \$1\.25/)).toBeInTheDocument();
    expect(
      within(region).getByText(/tool decisions: accept 5, reject 1/),
    ).toBeInTheDocument();
    expect(within(region).getByText(/failed: docs-server x1/)).toBeInTheDocument();
    expect(within(region).getByRole("button", { name: "Disable…" })).toBeEnabled();
    expect(within(region).queryByLabelText("Port")).toBeNull();
    const strip = screen.getByRole("group", { name: "Usage summary" });
    expect(within(strip).getByText("Listening")).toBeInTheDocument();
    expect(within(strip).getByText("http://127.0.0.1:4318")).toBeInTheDocument();
  });

  it("names a port that another program holds and the reason", async () => {
    bridge.set("obs otel status", {
      ...statusOn,
      receiver: {
        listening: false,
        state: "port-in-use",
        error: "port 4318 is used by another program",
      },
    });
    open();
    const region = await card();
    expect(await within(region).findByText("Port in use")).toBeInTheDocument();
    expect(
      within(region).getByText("port 4318 is used by another program"),
    ).toBeInTheDocument();
  });

  it("offers to apply again when the receiver is on but keys are gone", async () => {
    bridge.set("obs otel status", {
      ...statusOn,
      settings: {
        ...statusOn.settings,
        state: "partial",
        keys: { ...statusOn.settings.keys, OTEL_LOGS_EXPORTER: "differs" },
        warnings: [
          "env.OTEL_LOG_USER_PROMPTS is set in your Claude settings; Toolport never sets it",
        ],
      },
    });
    open();
    const region = await card();
    expect(await within(region).findByText("set to another value")).toBeInTheDocument();
    expect(within(region).getByText(/not every telemetry key/)).toBeInTheDocument();
    expect(within(region).getByText(/OTEL_LOG_USER_PROMPTS is set/)).toBeInTheDocument();
    expect(
      within(region).getByRole("button", { name: "Apply again on port 4318" }),
    ).toBeEnabled();
  });

  it("shows a loading state, then an error with Retry", async () => {
    const reply = deferred<unknown>();
    bridge.set("obs otel status", () => reply.promise);
    const { user } = open();
    expect(
      await screen.findByRole("status", { name: "Loading the receiver status" }),
    ).toBeInTheDocument();
    reply.resolve(failure("otel_status", "the Claude settings file is not valid JSON"));
    const alert = await within(await card()).findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't read the receiver status");
    expect(alert).toHaveTextContent("the Claude settings file is not valid JSON");
    expect(
      within(screen.getByRole("group", { name: "Usage summary" })).getByText("Unknown"),
    ).toBeInTheDocument();
    bridge.set("obs otel status", statusOn);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await within(cardNow()).findByText("Listening")).toBeInTheDocument();
  });

  it("says toolportctl could not run when the bridge is down", async () => {
    bridge.set("obs otel status", bridgeDown("toolportctl is not installed"));
    open();
    const alert = await within(await card()).findByRole("alert");
    expect(alert).toHaveTextContent("Toolport could not run toolportctl");
  });

  it("reads the status again on Check status", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    bridge.set("obs otel status", statusOn);
    await user.click(screen.getByRole("button", { name: "Check status" }));
    expect(await within(cardNow()).findByText("Listening")).toBeInTheDocument();
    expect(bridge.count("obs otel status")).toBe(2);
  });
});

describe("usage.otel-enable: Enable previews, confirms, applies", () => {
  async function typePort(user: ReturnType<typeof userEvent.setup>, port: string) {
    const field = within(await card()).getByLabelText("Port");
    await user.clear(field);
    await user.type(field, port);
  }

  it("previews with --dry-run, lists the keys, applies on confirm and shows the new state", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "4999");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    for (const action of ENABLE.actions as string[])
      expect(
        within(dialog).getByText(action, { normalizer: (t) => t }),
      ).toBeInTheDocument();
    expect(within(dialog).getByText(/http:\/\/127\.0\.0\.1:4999/)).toBeInTheDocument();
    expect(
      within(dialog).getAllByText(ENABLE.settingsPath as string).length,
    ).toBeGreaterThan(0);
    expect(
      within(dialog).getByText(/Restart running Claude Code sessions/),
    ).toBeInTheDocument();
    expect(within(dialog).getByText("toolportctl obs otel disable")).toBeInTheDocument();
    expect(
      within(dialog).getByText("toolportctl obs otel enable --port 4999"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("obs otel enable --port 4999 --dry-run");
    expect(bridge.ran()).not.toContain("obs otel enable --port 4999");
    expect(within(dialog).queryByLabelText(/Type/)).toBeNull();

    bridge.set("obs otel status", {
      ...statusOn,
      port: 4999,
      endpoint: "http://127.0.0.1:4999",
    });
    await user.click(within(dialog).getByRole("button", { name: "Enable receiver" }));
    await waitFor(() => expect(bridge.ran()).toContain("obs otel enable --port 4999"));
    expect(await within(cardNow()).findByText("Listening")).toBeInTheDocument();
    expect(within(cardNow()).getByText("http://127.0.0.1:4999")).toBeInTheDocument();
    expect(bridge.missing).toEqual([]);
  });

  it("leaves everything alone when the preview is cancelled", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "4999");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.ran()).not.toContain("obs otel enable --port 4999");
  });

  it("refuses a port that is not a number from 1 to 65535 before it runs anything", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "70000");
    expect(within(cardNow()).getByRole("button", { name: "Enable…" })).toBeDisabled();
    expect(within(cardNow()).getByRole("alert")).toHaveTextContent(
      "Use a whole number from 1 to 65535.",
    );
    await typePort(user, "0");
    expect(within(cardNow()).getByRole("button", { name: "Enable…" })).toBeDisabled();
    await typePort(user, "8080");
    expect(within(cardNow()).getByRole("button", { name: "Enable…" })).toBeEnabled();
    expect(bridge.ran().filter((line) => line.startsWith("obs otel enable"))).toEqual([]);
  });

  it("takes Enter in the port field as Enable…", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "4999{Enter}");
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(bridge.ran()).toContain("obs otel enable --port 4999 --dry-run");
  });

  it("shows the conflict the preview reports and applies nothing", async () => {
    bridge.set(
      "obs otel enable --port 4999 --dry-run",
      failure(
        "conflict",
        "/fixture/home/.claude/settings.json already sets OTEL_LOGS_EXPORTER to another value; not overwritten. Remove or change those keys, then run again",
      ),
    );
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "4999");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("conflict");
    expect(alert).toHaveTextContent("already sets OTEL_LOGS_EXPORTER to another value");
    expect(within(dialog).queryByRole("button", { name: "Enable receiver" })).toBeNull();
    expect(bridge.ran()).not.toContain("obs otel enable --port 4999");
  });

  it("shows the failure of the apply and does not claim the receiver is on", async () => {
    bridge.set(
      "obs otel enable --port 4999",
      failure("otel_enable", "could not write the settings file"),
    );
    const { user } = open();
    await within(await card()).findByText("Off");
    await typePort(user, "4999");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Enable receiver" }));
    expect(
      await screen.findByText("could not write the settings file"),
    ).toBeInTheDocument();
    expect(within(cardNow()).getByText("Off")).toBeInTheDocument();
  });

  it("does not run it when the registry does not know the command", async () => {
    const data = goldenData("commands") as { commands: Array<{ id: string }> };
    bridge.set("commands", {
      ...data,
      commands: data.commands.filter((row) => row.id !== "obs otel enable"),
    });
    const { user } = open();
    await within(await card()).findByText("Off");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    expect(
      await screen.findByText(/does not know how safe `obs otel enable` is/),
    ).toBeInTheDocument();
    expect(bridge.ran().some((line) => line.startsWith("obs otel enable"))).toBe(false);
  });
});

describe("usage.otel-disable: Disable previews, confirms, applies", () => {
  beforeEach(() => bridge.set("obs otel status", statusOn));

  it("previews the keys it removes, applies on confirm and shows the receiver off", async () => {
    const { user } = open();
    await within(await card()).findByText("Listening");
    await user.click(within(cardNow()).getByRole("button", { name: "Disable…" }));
    const dialog = await screen.findByRole("dialog");
    for (const action of DISABLE.actions as string[])
      expect(
        within(dialog).getByText(action, { normalizer: (t) => t }),
      ).toBeInTheDocument();
    expect(
      within(dialog).getByText("toolportctl obs otel enable --port 4318"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("obs otel disable --dry-run");
    expect(bridge.ran()).not.toContain("obs otel disable");
    expect(within(dialog).queryByLabelText(/Type/)).toBeNull();

    bridge.set("obs otel status", goldenData("obs-otel-status"));
    await user.click(within(dialog).getByRole("button", { name: "Disable receiver" }));
    await waitFor(() => expect(bridge.ran()).toContain("obs otel disable"));
    expect(await within(cardNow()).findByText("Off")).toBeInTheDocument();
    expect(
      within(cardNow()).getByRole("button", { name: "Enable…", hidden: true }),
    ).toBeInTheDocument();
  });

  it("asks for a typed confirmation when the registry marks the command destructive", async () => {
    bridge.set("commands", registryWithTier("obs otel disable", "destructive"));
    const { user } = open();
    await within(await card()).findByText("Listening");
    await user.click(within(cardNow()).getByRole("button", { name: "Disable…" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Disable receiver" });
    expect(confirm).toBeDisabled();
    await user.type(within(dialog).getByLabelText(/Type/), "disable");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    await waitFor(() => expect(bridge.ran()).toContain("obs otel disable"));
  });

  it("warns about a key that stays because it was changed since", async () => {
    bridge.set("obs otel disable --dry-run", {
      ...DISABLE,
      kept: ["OTEL_LOGS_EXPORTER"],
      actions: [
        ...(DISABLE.actions as string[]).slice(0, 3),
        "env.OTEL_LOGS_EXPORTER: kept, changed since Toolport set it",
      ],
    });
    const { user } = open();
    await within(await card()).findByText("Listening");
    await user.click(within(cardNow()).getByRole("button", { name: "Disable…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        "env.OTEL_LOGS_EXPORTER: kept, changed since Toolport set it",
      ),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByText(/env\.OTEL_LOGS_EXPORTER stays in your settings/),
    ).toBeInTheDocument();
  });
});

describe("OTel card: the preview is the dry run", () => {
  it("shows exactly the action lines of the golden dry runs, nothing else as a step", async () => {
    const { user } = open();
    await within(await card()).findByText("Off");
    const field = within(cardNow()).getByLabelText("Port");
    await user.clear(field);
    await user.type(field, "4999");
    await user.click(within(cardNow()).getByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    const steps = within(
      within(dialog).getByRole("list", { name: "Changes" }),
    ).getAllByRole("listitem");
    expect(steps.map((step) => step.textContent)).toEqual(
      (ENABLE.actions as string[]).map((action) => expect.stringContaining(action)),
    );
  });
});
