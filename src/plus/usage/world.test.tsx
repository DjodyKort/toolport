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
import { plusCtlFixtures } from "../fixtures/plusCtl";
import { usageCtlFixtures } from "./browserFixtures";
import { createBridge, goldenData, wire } from "./testkit";
import {
  createOtelWorld,
  emptyUsage,
  shiftDays,
  statusOff,
  statusOn,
  usageWorld,
} from "./world";

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
    const otel = createOtelWorld();
    expect(keys(otel.enable(4318, true))).toEqual(
      keys(goldenData("obs-otel-enable.preview")),
    );
    expect(keys(otel.enable(4318, false))).toEqual(
      keys(goldenData("obs-otel-enable.apply")),
    );
    expect(keys(otel.disable(true))).toEqual(
      keys(goldenData("obs-otel-disable.preview")),
    );
    expect(keys(otel.disable(false))).toEqual(keys(goldenData("obs-otel-disable.apply")));
    expect(keys(otel.status())).toEqual(keys(status));
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
    render(<UsageTab />);
    await screen.findByRole("group", { name: "Usage summary" });
    expect(screen.getByRole("region", { name: "By project" })).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Enable…" }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText("env.CLAUDE_CODE_ENABLE_TELEMETRY: added"),
    ).toBeInTheDocument();
  });
});

describe("the stateful OTel world", () => {
  it("changes only on an applied Enable or Disable, never on a preview", () => {
    const otel = createOtelWorld();
    expect(otel.status()).toMatchObject({ enabled: false, port: 4318 });
    otel.enable(4318, true);
    otel.disable(true);
    expect(otel.status().enabled).toBe(false);

    otel.enable(4999, false);
    const on = otel.status();
    expect(on).toMatchObject({
      enabled: true,
      port: 4999,
      endpoint: "http://127.0.0.1:4999",
      receiver: { listening: true, state: "listening" },
      settings: { state: "configured" },
      events: { count: 0, latest: null },
    });
    expect(Object.values(on.settings.keys)).toEqual(Array(5).fill("ok"));
    otel.disable(true);
    expect(otel.status().enabled).toBe(true);

    expect(otel.disable(false)).toMatchObject({ dryRun: false, port: 4999 });
    expect(otel.status()).toMatchObject({
      enabled: false,
      receiver: { state: "disabled" },
    });
    expect(Object.values(otel.status().settings.keys)).toEqual(Array(5).fill("missing"));
  });

  it("starts listening on request and keeps the stored events then", () => {
    expect(createOtelWorld({ enabled: true }).status()).toEqual(statusOn);
  });
});

describe("shiftDays", () => {
  it("moves every date by the same number of days and nothing else", () => {
    const world = usageWorld();
    const moved = shiftDays(world, "2026-12-30") as typeof world;
    expect(shiftDays(world, "2026-10-03")).toBe(world);
    const days = Object.keys(moved.byDay as object);
    expect(days[0]).toBe("2026-12-15");
    expect(days.at(-1)).toBe("2026-12-30");
    expect(Object.values(moved.byDay as object)).toEqual(
      Object.values(world.byDay as object),
    );
    expect(JSON.stringify(moved)).toContain("2026-12-29T11:00:01Z");
    expect(JSON.stringify(moved)).not.toContain("2026-10-");
  });
});

describe("the dev browser fixture of the Usage tab", () => {
  it("is what plusCtl serves, with no row taken over by another screen", () => {
    expect([...usageCtlFixtures.keys()]).toHaveLength(11);
    for (const [argv, reply] of usageCtlFixtures)
      expect(plusCtlFixtures.get(argv), argv).toBe(reply);
  });
});
