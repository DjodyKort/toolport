import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ctlShapes, envelopeShape } from "./data";
import { arr, bool, check, lit, nullable, num, obj, opt, rec, str } from "./shape";

const goldenDir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");
const stems = readdirSync(goldenDir)
  .filter((f) => f.endsWith(".json"))
  .map((f) => f.slice(0, -".json".length))
  .sort();
const golden = (stem: string) =>
  JSON.parse(readFileSync(join(goldenDir, `${stem}.json`), "utf8"));

/** Goldens whose `data` has no TS shape yet. A later worker types one, deletes the row here and
 * adds it to `ctlShapes`; `CTL_CONTRACT_STRICT=1` fails while any remain. */
const UNTYPED = new Set(stems.filter((stem) => !(stem in ctlShapes)));

describe("ctl envelope goldens", () => {
  it("are one wrapper of argv, exit code and a well-formed envelope", () => {
    expect(stems.length).toBeGreaterThan(20);
    for (const stem of stems) {
      const g = golden(stem);
      expect(Object.keys(g).sort(), stem).toEqual(["argv", "envelope", "exitCode"]);
      expect(check(envelopeShape, g.envelope, stem), stem).toEqual([]);
      expect(g.envelope.schemaVersion, stem).toBe(1);
      expect(g.envelope.ok, stem).toBe(g.exitCode === 0);
      if (!g.envelope.ok) expect(g.envelope.error.code, stem).toBeTruthy();
    }
  });

  it("match the TS shape of their data exactly (schema drift)", () => {
    for (const stem of stems.filter((s) => s in ctlShapes)) {
      const errors = check(ctlShapes[stem], golden(stem).envelope.data);
      expect(errors, `${stem}: update src/plus/bridge/data.ts`).toEqual([]);
    }
  });

  it("have a golden for every shape", () => {
    for (const stem of Object.keys(ctlShapes)) {
      expect(stems, `shape ${stem} has no golden`).toContain(stem);
    }
  });

  it("report the goldens that still have no TS shape", () => {
    const left = [...UNTYPED];
    if (process.env.CTL_CONTRACT_STRICT) {
      expect(left, "goldens without a TS shape").toEqual([]);
    } else {
      console.info(
        `ctl contract: ${stems.length - left.length} of ${stems.length} goldens have a TS shape; untyped: ${left.join(", ")}`,
      );
    }
  });
});

describe("shape checker", () => {
  const shape = obj({
    name: str,
    count: num,
    tags: arr(str),
    owner: nullable(str),
    note: opt(str),
    kind: lit("a", "b"),
    map: rec(num),
    nested: obj({ ok: bool }),
  });
  const good = () => ({
    name: "x",
    count: 1,
    tags: ["t"],
    owner: null,
    kind: "a",
    map: { k: 1 },
    nested: { ok: true },
  });

  it("accepts a conforming value, with or without an optional key", () => {
    expect(check(shape, good())).toEqual([]);
    expect(check(shape, { ...good(), note: "n" })).toEqual([]);
  });

  it("names a field the shape does not know, a missing field and a wrong type", () => {
    expect(check(shape, { ...good(), extra: 1 })).toEqual([
      "data.extra: not in the shape",
    ]);
    const without: Record<string, unknown> = good();
    delete without.name;
    expect(check(shape, without)).toEqual(["data.name: missing"]);
    expect(check(shape, { ...good(), count: "1" })).toEqual([
      "data.count: expected number, got string",
    ]);
    expect(check(shape, { ...good(), tags: ["t", 2] })).toEqual([
      "data.tags[1]: expected string, got number",
    ]);
    expect(check(shape, { ...good(), kind: "c" })[0]).toContain("expected one of a | b");
    expect(check(shape, { ...good(), nested: { ok: 1 } })).toEqual([
      "data.nested.ok: expected boolean, got number",
    ]);
    expect(check(shape, { ...good(), owner: 3 })).toEqual([
      "data.owner: expected string, got number",
    ]);
    expect(check(shape, [])).toEqual(["data: expected object, got array"]);
  });
});
