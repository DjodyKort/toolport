import { beforeEach, describe, expect, it, vi } from "vitest";
import { removalNeedsTyping, resetRemovalTier } from "./commands";

const ctl = vi.hoisted(() => ({ ctlData: vi.fn() }));

vi.mock("../bridge/ctl", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../bridge/ctl")>()),
  ctlData: ctl.ctlData,
}));

const policy = (tier: string) => ({ commands: [{ id: "secret rm", tier }] });

beforeEach(() => {
  resetRemovalTier();
  ctl.ctlData.mockReset();
});

describe("removing a secret", () => {
  it("test01_wants_the_key_typed_when_the_policy_tier_is_destructive", async () => {
    ctl.ctlData.mockResolvedValue(policy("destructive"));
    expect(await removalNeedsTyping()).toBe(true);
  });

  it("test02_wants_no_typing_when_the_policy_tier_is_lower", async () => {
    ctl.ctlData.mockResolvedValue(policy("write"));
    expect(await removalNeedsTyping()).toBe(false);
  });

  it("test03_wants_typing_when_the_policy_cannot_be_read_or_has_no_such_row", async () => {
    ctl.ctlData.mockRejectedValueOnce(new Error("bridge down"));
    expect(await removalNeedsTyping()).toBe(true);
    resetRemovalTier();
    ctl.ctlData.mockResolvedValue({ commands: [] });
    expect(await removalNeedsTyping()).toBe(true);
  });
});
