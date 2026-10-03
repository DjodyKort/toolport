import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, check } = vi.hoisted(() => ({ invoke: vi.fn(), check: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  BundleType: { Deb: "deb", Rpm: "rpm" },
  getBundleType: vi.fn(),
}));

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
    bundle: { createUpdaterArtifacts: boolean };
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

  it("is passed last to every fork build", () => {
    const ci = readFileSync(join(root, ".github/workflows/fork-ci.yml"), "utf8");
    const bundleAt = ci.indexOf("--config src-tauri/tauri.bundle.conf.json");
    const forkAt = ci.indexOf("--config src-tauri/tauri.fork.conf.json");
    expect(bundleAt).toBeGreaterThan(-1);
    expect(forkAt).toBeGreaterThan(bundleAt);
    const pkg = readJson("package.json") as { scripts: Record<string, string> };
    expect(pkg.scripts["tauri:fork"]).toContain(
      "--config src-tauri/tauri.fork.conf.json",
    );
  });
});

describe("fork egress switch", () => {
  beforeEach(() => {
    vi.stubEnv("VITE_TOOLPORT_UPSTREAM_EGRESS", "");
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    check.mockReset();
  });
  afterEach(() => vi.unstubAllEnvs());

  const upstream = [
    "https://github.com/btsouth/toolport/releases/latest",
    "https://github.com/btsouth/toolport",
    "https://toolport.app/teams#pricing",
    "https://teams.toolport.app/?intent=create-team",
  ];

  it("openExternal refuses every upstream URL", async () => {
    const { openExternal } = await import("@/lib/openUrl");
    for (const url of upstream) await openExternal(url);
    expect(invoke).not.toHaveBeenCalled();
    await openExternal("https://github.com/DjodyKort/toolport");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("never asks the updater plugin for an update", async () => {
    const { checkForUpdate } = await import("@/lib/updater");
    await expect(checkForUpdate()).resolves.toEqual({ kind: "current" });
    expect(check).not.toHaveBeenCalled();
  });

  it("rejects the hosted Teams server as a team URL", async () => {
    const { teamUrlError } = await import("@/lib/teamUrl");
    expect(teamUrlError("https://teams.toolport.app")).not.toBeNull();
    expect(teamUrlError("https://teams.example.com")).toBeNull();
  });

  it("restores upstream behaviour only when the build flag is set", async () => {
    const { forkEgressDisabled } = await import("@/lib/fork");
    expect(forkEgressDisabled()).toBe(true);
    vi.stubEnv("VITE_TOOLPORT_UPSTREAM_EGRESS", "1");
    expect(forkEgressDisabled()).toBe(false);
  });
});

describe("upstream URL literals", () => {
  // Each file here either guards its use behind the fork switch (brand.rs,
  // fork.ts, openUrl/updater/starPrompt/teamUrl callers, share/teams controllers),
  // only mentions the URL in prose or tests, or is the OAuth CIMD document, which
  // authorization servers fetch and the app never requests.
  const allowed = new Set([
    "src/components/ActivityView.tsx",
    "src/components/AppSidebar.tsx",
    "src/components/ShareDialog.tsx",
    "src/lib/api.ts",
    "src/lib/fork.ts",
    "src/lib/starPrompt.ts",
    "src/lib/teamsPlan.ts",
    "src/lib/teamUrl.ts",
    "src/lib/updater.ts",
    "src-tauri/src/bin/toolport-gateway.rs",
    "src-tauri/src/brand.rs",
    "src-tauri/src/catalog.rs",
    "src-tauri/src/gateway_publish.rs",
    "src-tauri/src/linux_native/mod.rs",
    "src-tauri/src/linux_native/teams.rs",
    "src-tauri/src/oauth.rs",
    "src-tauri/src/sharing_controller.rs",
    "src-tauri/src/teams_plan.rs",
    "src-tauri/src/teams.rs",
  ]);

  function walk(dir: string, out: string[] = []): string[] {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path, out);
      else if (/\.(ts|tsx|rs)$/.test(name) && !/\.test\./.test(name)) out.push(path);
    }
    return out;
  }

  it("appear only in reviewed files", () => {
    const hits = [...walk(join(root, "src")), ...walk(join(root, "src-tauri/src"))]
      .filter((path) => !path.includes(`${join("src", "test")}`))
      .filter((path) =>
        /toolport\.app|github\.com\/btsouth/.test(readFileSync(path, "utf8")),
      )
      .map((path) => relative(root, path))
      .filter((path) => !allowed.has(path));
    expect(hits).toEqual([]);
  });

  it("keeps the CIMD client document", () => {
    const oauth = readFileSync(join(root, "src-tauri/src/oauth.rs"), "utf8");
    expect(oauth).toContain(
      "https://toolport.app/.well-known/oauth-client/toolport.json",
    );
  });
});
