import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  resourceGoldenShape,
  resourceShapes,
  selfmcpToolShapes,
  toolErrorShape,
  toolGoldenShape,
} from "../types";
import { check } from "./shape";

const goldenDir = join(__dirname, "../../../src-tauri/tests/fixtures/selfmcp-envelopes");
const stems = readdirSync(goldenDir)
  .filter((f) => f.endsWith(".json"))
  .map((f) => f.slice(0, -".json".length))
  .sort();
const golden = (stem: string) =>
  JSON.parse(readFileSync(join(goldenDir, `${stem}.json`), "utf8"));

const resourceStems = stems.filter((stem) => stem.startsWith("resource-"));
const toolStems = stems.filter((stem) => !stem.startsWith("resource-"));
const called = (stem: string) => golden(stem).tool as string;

describe("self-MCP tool goldens", () => {
  it("are one call and its result, and a failed call carries the error text", () => {
    expect(toolStems.length).toBeGreaterThan(80);
    for (const stem of toolStems) {
      const g = golden(stem);
      expect(check(toolGoldenShape, g, stem), stem).toEqual([]);
      expect(stem.split(".")[0], stem).toBe(g.tool);
      expect("text" in g, `${stem}: text only on failure`).toBe(g.isError);
      if (g.isError)
        expect(g.text, stem).toMatch(new RegExp(`^${g.result.error.kind}: `));
    }
  });

  it("match the TS shape of their result exactly (schema drift)", () => {
    for (const stem of toolStems) {
      const g = golden(stem);
      const shape = g.isError ? toolErrorShape : selfmcpToolShapes[g.tool];
      expect(shape, `${stem}: no shape for ${g.tool}`).toBeDefined();
      expect(
        check(shape, g.result, "result"),
        `${stem}: update src/plus/types/selfmcp-*.ts`,
      ).toEqual([]);
    }
  });

  it("have a successful golden for every shape", () => {
    const succeeded = new Set(toolStems.filter((s) => !golden(s).isError).map(called));
    for (const tool of Object.keys(selfmcpToolShapes)) {
      expect(succeeded.has(tool), `shape ${tool} has no successful golden`).toBe(true);
    }
    for (const tool of succeeded) {
      expect(selfmcpToolShapes, `no TS shape for ${tool}`).toHaveProperty(tool);
    }
  });
});

describe("self-MCP resource goldens", () => {
  it("are a uri, a media type and a body, and the body matches its TS shape", () => {
    expect(resourceStems.length).toBeGreaterThan(10);
    for (const stem of resourceStems) {
      const g = golden(stem);
      expect(check(resourceGoldenShape, g, stem), stem).toEqual([]);
      const body = g.mimeType === "application/json" ? "json" : "text";
      const other = body === "json" ? "text" : "json";
      expect(body in g && !(other in g), `${stem}: carries ${body} only`).toBe(true);
      const shape = resourceShapes[stem];
      expect(shape, `no shape for ${stem}`).toBeDefined();
      expect(check(shape, g.json ?? g.text, "body"), stem).toEqual([]);
    }
    expect(Object.keys(resourceShapes).sort()).toEqual(resourceStems);
  });
});
