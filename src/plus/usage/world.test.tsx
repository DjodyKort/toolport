import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { UsageTab } from "./UsageTab";
import { usageCtlFixtures } from "./browserFixtures";
import { createBridge, goldenData, wire } from "./testkit";
import { emptyUsage, statusOff, statusOn, usageWorld } from "./world";

const keys = (value: unknown) => Object.keys(value as object).sort();
const first = (value: unknown) => Object.values(value as object)[0];

describe("the synthetic index has the shape of the real envelopes", () => {
  const golden = goldenData("usage.apply");

  for (const [name, data] of [
    ["usageWorld", usageWorld()],
    ["emptyUsage", emptyUsage()],
  ] as const) {
    it(`${name} has the members of the golden usage envelope`, () => {
      const world = data as Record<string, unknown>;
      expect(keys(world)).toEqual(keys(golden));
      expect(keys(world.otel)).toEqual(keys(golden.otel));
      expect(
        keys(world.otel && (world.otel as Record<string, unknown>).apiRequests),
      ).toEqual(keys((golden.otel as Record<string, unknown>).apiRequests));
      expect(keys(world.sources)).toEqual(keys(golden.sources));
      expect(keys(world.index)).toEqual(keys(golden.index));
      expect(keys(world.totals)).toEqual(keys(golden.totals));
    });
  }

  it("has day, session, model and server rows like the golden ones", () => {
    const world = usageWorld();
    expect(keys(first(world.byDay))).toEqual(keys(first(golden.byDay)));
    expect(keys(first(world.byModel))).toEqual(keys(first(golden.byModel)));
    expect(keys(first(world.bySession))).toEqual(keys(first(golden.bySession)));
    expect(keys(first(world.byMcpServer))).toEqual(keys(first(golden.byMcpServer)));
  });

  it("has OTel status and write data with the members of the goldens", () => {
    const status = goldenData("obs-otel-status");
    for (const world of [statusOn, statusOff]) {
      expect(keys(world)).toEqual(keys(status));
      expect(keys(world.settings)).toEqual(keys(status.settings));
      expect(keys(world.settings.keys)).toEqual(
        keys((status.settings as { keys: unknown }).keys),
      );
      expect(keys(world.receiver)).toEqual(keys(status.receiver));
      expect(keys(world.events)).toEqual(keys(status.events));
    }
    const enable = usageCtlFixtures.get("obs otel enable --port 4318 --dry-run");
    expect(keys(enable)).toEqual(keys(goldenData("obs-otel-enable.preview")));
    expect(keys(usageCtlFixtures.get("obs otel enable --port 4318"))).toEqual(
      keys(goldenData("obs-otel-enable.apply")),
    );
    expect(keys(usageCtlFixtures.get("obs otel disable --dry-run"))).toEqual(
      keys(goldenData("obs-otel-disable.preview")),
    );
    expect(keys(usageCtlFixtures.get("obs otel disable"))).toEqual(
      keys(goldenData("obs-otel-disable.apply")),
    );
  });
});

describe("the dev browser fixtures drive the tab", () => {
  beforeEach(() => {
    const bridge = createBridge();
    for (const [argv, reply] of usageCtlFixtures) bridge.set(argv, reply);
    wire(mocks, bridge);
  });

  it("shows the usage of the fixture and previews Enable on the default port", async () => {
    const user = userEvent.setup();
    render(<UsageTab today="2026-10-04" />);
    await screen.findByRole("group", { name: "Usage summary" });
    expect(screen.getByRole("region", { name: "By project" })).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText("env.CLAUDE_CODE_ENABLE_TELEMETRY: added"),
    ).toBeInTheDocument();
  });
});
