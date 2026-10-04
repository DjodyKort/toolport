import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { plusCtlFixtures } from "./plusCtl";
import { plusInvokeFixtures } from "./plusInvoke";
import { ctlCallIsCovered, ctlCalls, plusInvokeNames, scanPlusSources } from "./scan";

const plusDir = join(__dirname, "..");

describe("plus_invoke browser fixtures", () => {
  it("has a reply for every plus command the frontend calls", () => {
    const { invoked } = scanPlusSources(plusDir);
    expect(invoked).toContain("plus.auth.rows");
    for (const command of invoked) {
      expect(plusInvokeFixtures.has(command), command).toBe(true);
    }
  });

  it("has a reply for every toolportctl command the frontend runs", () => {
    const { ctl } = scanPlusSources(plusDir);
    for (const call of ctl) {
      expect(
        ctlCallIsCovered(call, plusCtlFixtures.keys()),
        `plus_ctl ${call.words.join(" ")}`,
      ).toBe(true);
    }
  });
});

describe("fixture coverage scanner", () => {
  it("reads names with any number of segments, camelCase, digits and underscores", () => {
    const source = [
      'invoke("plus.ping")',
      "invoke('plus.client.directAdd')",
      "invoke(`plus.import_mcpm.renameRefs`)",
      'invoke("plus.sync.v2Reset")',
      'const prefix = "plus.auth.";',
    ].join("\n");
    expect(plusInvokeNames(source)).toEqual([
      "plus.ping",
      "plus.client.directAdd",
      "plus.import_mcpm.renameRefs",
      "plus.sync.v2Reset",
    ]);
  });

  it("finds toolportctl argv with literal and dynamic parts", () => {
    const source = [
      'await ctlData(["status"]);',
      'ctlData<Row[]>(["skills", "ls", "--json"]);',
      'runCtl(["server", "info", name], {});',
      "ctlData(argv);",
    ].join("\n");
    expect(ctlCalls(source)).toEqual([
      { words: ["status"], complete: true },
      { words: ["skills", "ls", "--json"], complete: true },
      { words: ["server", "info"], complete: false },
    ]);
    expect(
      ctlCallIsCovered({ words: ["server", "info"], complete: false }, ["server info a"]),
    ).toBe(true);
    expect(
      ctlCallIsCovered({ words: ["server", "info"], complete: true }, ["server info a"]),
    ).toBe(false);
  });

  it("walks nested folders and skips tests and fixtures", () => {
    const root = mkdtempSync(join(tmpdir(), "plus-scan-"));
    try {
      mkdirSync(join(root, "screens/deep"), { recursive: true });
      mkdirSync(join(root, "fixtures"));
      writeFileSync(join(root, "screens/deep/A.tsx"), 'invoke("plus.deep.nestedName");');
      writeFileSync(join(root, "screens/A.test.tsx"), 'invoke("plus.in.tests");');
      writeFileSync(join(root, "fixtures/f.ts"), 'invoke("plus.in.fixtures");');
      const { invoked } = scanPlusSources(root);
      expect([...invoked]).toEqual(["plus.deep.nestedName"]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
