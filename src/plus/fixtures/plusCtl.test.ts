import { describe, expect, it } from "vitest";
import { plusCtlCancel, plusCtlFixtures, plusCtlResult, plusCtlStart } from "./plusCtl";

describe("plus_ctl browser fixtures", () => {
  it("answers a known argv with an envelope and hands a result over once", () => {
    const job = plusCtlStart(["status"]);
    const result = plusCtlResult(job);
    expect(result.envelope).toMatchObject({
      ok: true,
      command: "status",
      schemaVersion: 1,
      data: plusCtlFixtures.get("status"),
    });
    expect(() => plusCtlResult(job)).toThrow(/unknown job/);
  });

  it("rejects an argv without a fixture row, like plus_invoke does", () => {
    expect(() => plusCtlStart(["frobnicate"])).toThrow(/Unimplemented fixture command/);
  });

  it("forgets a cancelled job", () => {
    const job = plusCtlStart(["status"]);
    expect(plusCtlCancel(job)).toBeNull();
    expect(() => plusCtlResult(job)).toThrow(/unknown job/);
  });
});
