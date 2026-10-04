import { describe, expect, it } from "vitest";
import { check } from "../bridge/shape";
import { sourcesLsData, sourcesRootLsData } from "../bridge/data";
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

  it("serves sources fixtures that have the shape of the CLI output", () => {
    for (const argv of [
      ["sources", "ls"],
      ["sources", "ls", "--items"],
    ]) {
      const data = plusCtlResult(plusCtlStart(argv)).envelope?.data;
      expect(check(sourcesLsData, data), argv.join(" ")).toEqual([]);
    }
    const roots = plusCtlResult(plusCtlStart(["sources", "root", "ls"])).envelope?.data;
    expect(check(sourcesRootLsData, roots)).toEqual([]);
  });

  it("shows the Mac case in the sources fixture: ODH behind, org stamp, duplicate library", () => {
    const data = plusCtlFixtures.get(
      "sources ls --items",
    ) as typeof import("./sources").plusSourcesItemsFixture;
    const byId = new Map(data.sources.map((s) => [s.id, s]));
    expect(data.sources).toHaveLength(10);
    expect(byId.get("repo:odh")?.freshness).toMatchObject({
      behind: 114,
      inCheckout: false,
    });
    expect(byId.get("org")?.freshness?.lastSync).toBeTruthy();
    expect(byId.get("library")?.status.state).toBe("duplicate");
  });
});
