import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { CompressionTab } from "./CompressionTab";
import { goldenData, statusData } from "./fixtures";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

describe("Compression tab: status", () => {
  it("compression.status: reproduces today's state: rtk-only, hook runtime, interactive preset, healthy engine", async () => {
    render(<CompressionTab />);
    const strip = await screen.findByLabelText("Compression status");
    expect(within(strip).getByText("rtk-only")).toBeInTheDocument();
    expect(within(strip).getByText("hook runtime")).toBeInTheDocument();
    expect(within(strip).getByText("interactive")).toBeInTheDocument();
    expect(within(strip).getByText("cache mode · port 8787")).toBeInTheDocument();
    expect(within(strip).getByText("Healthy")).toBeInTheDocument();
    expect(within(strip).getByText("0.29.0")).toBeInTheDocument();
  });

  it("compression.status: shows drift when the installed engine differs from the pin", async () => {
    const golden = statusData();
    bridge.set("compression status", {
      ...golden,
      provider: "headroom",
      runtime: "proxy",
      pin: { ...golden.pin, installed: "0.30.1", drift: true },
    });
    render(<CompressionTab />);
    expect(await screen.findByText("Drift")).toBeInTheDocument();
    expect(screen.getByText("0.30.1 installed, pin 0.29.0")).toBeInTheDocument();
  });

  it("compression.status: shows a loading state, then an error with Retry that reads again", async () => {
    const user = userEvent.setup();
    bridge.set(
      "compression status",
      failure("config_invalid", "compression.json is not valid JSON"),
    );
    render(<CompressionTab />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "compression.json is not valid JSON",
    );
    bridge.set("compression status", statusData());
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    expect(await screen.findByLabelText("Compression status")).toBeInTheDocument();
    expect(bridge.count("compression status")).toBe(2);
  });
});

describe("Compression tab: provider switch", () => {
  it("compression.set-provider: previews with set-provider --dry-run, shows its action lines, applies on confirm", async () => {
    const user = userEvent.setup();
    render(<CompressionTab />);
    await user.click(await screen.findByRole("button", { name: "Switch to headroom" }));
    const dialog = await screen.findByRole("dialog");
    const dry = goldenData("compression-set-provider.preview");
    for (const action of dry.actions as string[]) {
      expect(
        within(dialog).getByText(action, { normalizer: (text) => text }),
      ).toBeInTheDocument();
    }
    for (const warning of dry.warnings as string[]) {
      expect(within(dialog).getByText(warning)).toBeInTheDocument();
    }
    expect(bridge.ran()).toContain("compression set-provider headroom --dry-run");
    expect(bridge.ran()).not.toContain("compression set-provider headroom");
    expect(
      within(dialog).getByText("toolportctl compression set-provider rtk-only"),
    ).toBeInTheDocument();

    bridge.set("compression status", {
      ...statusData(),
      provider: "headroom",
      runtime: "proxy",
    });
    await user.click(within(dialog).getByRole("button", { name: "Switch provider" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("compression set-provider headroom"),
    );
    expect(await screen.findByText("Done")).toBeInTheDocument();
    await waitFor(() => expect(bridge.count("compression status")).toBe(2));
  });

  it("compression.set-provider: applies nothing when the preview is cancelled", async () => {
    const user = userEvent.setup();
    render(<CompressionTab />);
    await user.click(await screen.findByRole("button", { name: "Switch to headroom" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(bridge.ran()).not.toContain("compression set-provider headroom");
  });

  it("compression.set-provider: shows the CLI's error when the preview fails", async () => {
    const user = userEvent.setup();
    bridge.set(
      "compression set-provider headroom --dry-run",
      failure("usage", "set-provider needs a provider"),
    );
    render(<CompressionTab />);
    await user.click(await screen.findByRole("button", { name: "Switch to headroom" }));
    expect(await screen.findByText("set-provider needs a provider")).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("compression set-provider headroom");
  });
});

describe("Compression tab: presets", () => {
  it("compression.presets: lists the presets with the active one marked and no Use on it", async () => {
    render(<CompressionTab />);
    const list = await screen.findByRole("region", { name: "Presets" });
    expect(await within(list).findByText("agent")).toBeInTheDocument();
    expect(within(list).getByText("active")).toBeInTheDocument();
    expect(within(list).queryByRole("button", { name: "Use interactive" })).toBeNull();
    expect(within(list).getByText(/token · agent-90 · port 8788/)).toBeInTheDocument();
  });

  it("compression.use: previews with use --dry-run and applies on confirm", async () => {
    const user = userEvent.setup();
    render(<CompressionTab />);
    await user.click(await screen.findByRole("button", { name: "Use agent" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText("would save config (provider=none)"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("compression use agent --dry-run");
    await user.click(within(dialog).getByRole("button", { name: "Use preset" }));
    await waitFor(() => expect(bridge.ran()).toContain("compression use agent"));
  });
});
