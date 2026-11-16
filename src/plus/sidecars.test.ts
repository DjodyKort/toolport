import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = join(__dirname, "..", "..");
const read = (path: string) => readFileSync(join(root, path), "utf8");

const GATEWAY = "toolport-gateway";

function stagedPlusNames(): string[] {
  const match = read("scripts/fork/prepare-plus-sidecar.mjs").match(
    /PLUS_SIDECARS = \[([^\]]*)\]/,
  );
  expect(match).not.toBeNull();
  return [...match![1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

describe("fork sidecars", () => {
  const plus = stagedPlusNames();

  it("stages exactly the binaries plus/bins builds", () => {
    const bins = [...read("plus/bins/Cargo.toml").matchAll(/^name = "([^"]+)"$/gm)]
      .map((m) => m[1])
      .filter((name) => name !== "toolport-plus-bins");
    expect([...plus].sort()).toEqual(bins.sort());
  });

  it("lists the gateway and every staged binary as externalBin in the overlay", () => {
    const overlay = JSON.parse(read("src-tauri/tauri.fork.conf.json"));
    expect(overlay.bundle.externalBin).toEqual([
      `binaries/${GATEWAY}`,
      ...plus.map((name) => `binaries/${name}`),
    ]);
  });

  it("stages the plus binaries before the gateway build in beforeBuildCommand", () => {
    const overlay = JSON.parse(read("src-tauri/tauri.fork.conf.json"));
    expect(overlay.build.beforeBuildCommand).toBe(
      "node scripts/fork/prepare-plus-sidecar.mjs && npm run build:bundle",
    );
  });

  it("signs the same helpers fork-codesign.sh expects", () => {
    const helpers = read("scripts/fork/fork-codesign.sh").match(
      /^EXPECTED_HELPERS="([^"]+)"$/m,
    );
    expect(helpers).not.toBeNull();
    expect(helpers![1].split(" ").sort()).toEqual([GATEWAY, ...plus].sort());
  });
});
