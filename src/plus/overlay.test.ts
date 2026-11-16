import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const root = join(__dirname, "..", "..");
const readJson = (path: string) => JSON.parse(readFileSync(join(root, path), "utf8"));

type Json = Record<string, unknown>;

// RFC 7396, the merge Tauri applies to every --config argument.
function mergePatch(target: Json, patch: Json): Json {
  const out: Json = { ...target };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) delete out[key];
    else if (typeof value === "object" && !Array.isArray(value)) {
      const current = out[key];
      out[key] = mergePatch(
        current && typeof current === "object" && !Array.isArray(current)
          ? (current as Json)
          : {},
        value as Json,
      );
    } else out[key] = value;
  }
  return out;
}

describe("tauri.fork.conf.json overlay", () => {
  const base = readJson("src-tauri/tauri.conf.json") as Json;
  const bundle = readJson("src-tauri/tauri.bundle.conf.json") as Json;
  const fork = readJson("src-tauri/tauri.fork.conf.json") as Json;
  const merged = mergePatch(mergePatch(base, bundle), fork) as {
    identifier: string;
    bundle: { createUpdaterArtifacts: boolean; externalBin: string[] };
    plugins: { updater: { pubkey: string; endpoints: string[] } };
  };

  it("uses its own identifier and leaves the base config alone", () => {
    expect(base.identifier).toBe("com.tsout.conduit");
    expect(merged.identifier).toBe("com.djodykort.toolportplus");
  });

  it("cannot self-update into upstream releases", () => {
    const updater = merged.plugins.updater;
    expect(updater.endpoints).toEqual([]);
    expect(updater.pubkey).toBe("");
    expect(merged.bundle.createUpdaterArtifacts).toBe(false);
    expect(JSON.stringify(merged)).not.toMatch(/btsouth|toolport\.app/);
  });

  it("carries no keychain access group in the fork entitlements", () => {
    const plist = readFileSync(join(root, "src-tauri/Fork.entitlements.plist"), "utf8");
    expect(plist).not.toContain("keychain-access-groups");
    expect(plist).not.toContain("application-identifier");
  });
});
