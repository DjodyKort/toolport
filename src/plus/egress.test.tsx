import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, check } = vi.hoisted(() => ({ invoke: vi.fn(), check: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  BundleType: { Deb: "deb", Rpm: "rpm" },
  getBundleType: vi.fn(),
}));

import { isEgressRefused, upstreamEgressDisabled } from "@/plus/egress";
import { openExternal } from "@/lib/openUrl";
import { readStarStage } from "@/lib/starPrompt";
import { teamUrlError } from "@/lib/teamUrl";
import { checkForUpdate } from "@/lib/updater";

const BLOCKED = [
  "https://toolport.app/api/share?id=abc",
  "https://teams.toolport.app/",
  "https://TOOLPORT.APP/teams#pricing",
  "https://updates.toolport.app./x",
  "https://github.com/btsouth/toolport/releases/latest",
  "  https://github.com/BTSouth/toolport  ",
];

const ALLOWED = [
  "https://github.com/DjodyKort/toolport/releases",
  "https://github.com/someone/btsouth",
  "https://teams.example.com",
  "https://notoolport.app/",
  "https://toolport.app.example.com/",
  "not a url",
];

describe("fork egress guards", () => {
  beforeEach(() => {
    vi.stubEnv("MODE", "production");
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    check.mockReset();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("classifies upstream hosted URLs", () => {
    expect(upstreamEgressDisabled()).toBe(true);
    for (const url of BLOCKED) expect(isEgressRefused(url), url).toBe(true);
    for (const url of ALLOWED) expect(isEgressRefused(url), url).toBe(false);
  });

  it("leaves upstream behaviour alone in test mode", () => {
    vi.stubEnv("MODE", "test");
    expect(upstreamEgressDisabled()).toBe(false);
    for (const url of BLOCKED) expect(isEgressRefused(url), url).toBe(false);
  });

  it("openExternal refuses every upstream URL and opens the rest", async () => {
    for (const url of BLOCKED) await openExternal(url);
    expect(invoke).not.toHaveBeenCalled();
    await openExternal("https://github.com/DjodyKort/toolport");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("never asks the updater plugin for an update", async () => {
    await expect(checkForUpdate()).resolves.toEqual({ kind: "current" });
    expect(check).not.toHaveBeenCalled();
  });

  it("rejects the hosted Teams server as a team URL", () => {
    expect(teamUrlError("https://teams.toolport.app")).toMatch(/disabled in this build/);
    expect(teamUrlError("https://teams.example.com")).toBeNull();
  });

  it("never schedules the star prompt", () => {
    localStorage.clear();
    vi.stubEnv("MODE", "test");
    expect(readStarStage()).toBe("card");
    vi.stubEnv("MODE", "production");
    expect(readStarStage()).toBe("done");
  });
});
