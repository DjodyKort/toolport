import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { plusInvokeFixtures } from "./plusInvoke";

const plusDir = join(__dirname, "..");

function plusCommandsInvokedBy(file: string): string[] {
  const source = readFileSync(join(plusDir, file), "utf8");
  return [...source.matchAll(/"(plus\.[A-Za-z]+(?:\.[A-Za-z]+)+)"/g)].map((m) => m[1]);
}

describe("plus_invoke browser fixtures", () => {
  it("has a reply for every plus command the frontend calls", () => {
    const sources = readdirSync(plusDir).filter(
      (f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f),
    );
    const called = new Set(sources.flatMap(plusCommandsInvokedBy));
    expect(called).toContain("plus.auth.rows");
    for (const command of called) {
      expect(plusInvokeFixtures.has(command), command).toBe(true);
    }
  });
});
