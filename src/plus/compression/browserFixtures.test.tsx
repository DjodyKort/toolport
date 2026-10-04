import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { ctlShapes } from "../bridge/data";
import { check } from "../bridge/shape";
import { plusCtlFixtures, plusCtlResult, plusCtlStart } from "../fixtures/plusCtl";
import { compressionLedgerSummaryData, compressionShapes } from "../types/compression";
import { compressionBrowserFixtures } from "./browserFixtures";
import { CompressionTab } from "./CompressionTab";

beforeEach(() => {
  listen.mockReset().mockResolvedValue(() => {});
  invoke
    .mockReset()
    .mockImplementation(async (command: string, args: Record<string, unknown> = {}) => {
      if (command === "plus_ctl") return plusCtlStart(args.argv as string[]);
      if (command === "plus_ctl_result") return plusCtlResult(args.job as string);
      if (command === "plus_ctl_cancel") return null;
      throw new Error(`unexpected invoke ${command}`);
    });
});

describe("Compression browser fixtures", () => {
  it("are served by the dev plus_ctl fixture", () => {
    for (const [argv] of compressionBrowserFixtures)
      expect(plusCtlFixtures.has(argv), argv).toBe(true);
  });

  it("have the shape of the real command output", () => {
    const stems: Record<string, string> = {
      "compression status": "compression-status",
      "compression presets": "compression-presets",
      "compression pin": "compression-pin",
      "compression doctor": "compression-doctor",
      "compression seal --dry-run": "compression-seal.preview",
      "compression seal --apply": "compression-seal.apply",
      "compression sync": "compression-sync.apply",
      "compression update --latest": "compression-update.preview",
      "compression verify": "compression-verify.measured",
    };
    for (const [argv, stem] of Object.entries(stems))
      expect(
        check({ ...ctlShapes, ...compressionShapes }[stem], plusCtlFixtures.get(argv)),
        argv,
      ).toEqual([]);
    expect(
      check(
        compressionLedgerSummaryData,
        plusCtlFixtures.get("compression ledger summary"),
      ),
    ).toEqual([]);
  });

  it("draw the whole tab: state, pin, health, a ledger with two providers", async () => {
    render(<CompressionTab />);
    const strip = await screen.findByLabelText("Compression status");
    expect(within(strip).getByText("rtk-only")).toBeInTheDocument();
    const ledger = await screen.findByRole("region", { name: "Savings ledger" });
    expect(await within(ledger).findByRole("img")).toHaveAccessibleName(/headroom/);
    expect(
      await screen.findByText("headroom-ai[proxy,code,ml]==0.29.0"),
    ).toBeInTheDocument();
  });

  it("preview a provider switch and a seal like the real dry run", async () => {
    const user = userEvent.setup();
    render(<CompressionTab />);
    await user.click(await screen.findByRole("button", { name: "Switch to headroom" }));
    expect(
      await screen.findByRole("dialog", { name: /Switch the provider/ }),
    ).toBeInTheDocument();
  });
});
