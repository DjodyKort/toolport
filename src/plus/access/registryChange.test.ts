import { describe, expect, it } from "vitest";
import type { Registry } from "@/lib/types";
import { applyChange, changeOf } from "./registryChange";
import type { WriteOutcome } from "./data";

const registry = {
  version: 3,
  servers: [],
  profiles: [
    { id: "work", name: "Work", enabledServerIds: ["a"], toolScope: { a: ["x"] } },
    { id: "focus", name: "Focus", enabledServerIds: ["b"] },
  ],
  activeProfileId: null,
  clientScopes: { "claude-code": "work", zed: "Focus", cursor: "focus" },
  folderProfiles: [
    { path: "/w/a", profile: "work" },
    { path: "/w/b", profile: "focus" },
  ],
  clientDiscovery: { zed: "lazy" },
} as unknown as Registry;

describe("the registry after an access write", () => {
  it("test01_a_new_set_is_listed_with_its_servers", () => {
    const next = applyChange(registry, {
      kind: "set-added",
      id: "deep",
      name: "Deep",
      serverIds: ["a"],
    });
    expect(next.profiles.at(-1)).toEqual({
      id: "deep",
      name: "Deep",
      enabledServerIds: ["a"],
    });
  });

  it("test02_deleting_with_detach_drops_the_scopes_and_routes_that_named_it_by_id_or_name", () => {
    const next = applyChange(registry, {
      kind: "set-removed",
      id: "focus",
      detach: true,
    });
    expect(next.profiles.map((p) => p.id)).toEqual(["work"]);
    expect(next.clientScopes).toEqual({ "claude-code": "work" });
    expect(next.folderProfiles).toEqual([{ path: "/w/a", profile: "work" }]);
  });

  it("test03_deleting_and_leaving_keeps_what_named_it", () => {
    const next = applyChange(registry, {
      kind: "set-removed",
      id: "focus",
      detach: false,
    });
    expect(next.clientScopes).toEqual(registry.clientScopes);
    expect(next.folderProfiles).toEqual(registry.folderProfiles);
  });

  it("test04_a_client_is_bound_and_unbound", () => {
    const bound = applyChange(registry, {
      kind: "client-scope",
      client: "windsurf",
      setId: "work",
    });
    expect(bound.clientScopes?.windsurf).toBe("work");
    const unbound = applyChange(registry, {
      kind: "client-scope",
      client: "zed",
      setId: null,
    });
    expect(unbound.clientScopes).not.toHaveProperty("zed");
  });

  it("test05_a_route_replaces_the_one_of_the_same_folder_and_a_removal_drops_it", () => {
    const moved = applyChange(registry, {
      kind: "folder-route",
      path: "/w/a",
      setId: "focus",
    });
    expect(moved.folderProfiles).toEqual([
      { path: "/w/b", profile: "focus" },
      { path: "/w/a", profile: "focus" },
    ]);
    const gone = applyChange(registry, {
      kind: "folder-route",
      path: "/w/b",
      setId: null,
    });
    expect(gone.folderProfiles).toEqual([{ path: "/w/a", profile: "work" }]);
  });

  it("test06_the_default_the_tools_and_the_discovery_follow_the_write", () => {
    expect(
      applyChange(registry, { kind: "default", id: "work" }).defaultAccessProfileId,
    ).toBe("work");
    const narrowed = applyChange(registry, {
      kind: "tools",
      setId: "work",
      serverId: "a",
      tools: null,
    });
    expect(narrowed.profiles[0].toolScope).toEqual({});
    const pinned = applyChange(registry, {
      kind: "discovery",
      client: "cursor",
      mode: "grouped",
    });
    expect(pinned.clientDiscovery).toEqual({ zed: "lazy", cursor: "grouped" });
    const inherit = applyChange(registry, {
      kind: "discovery",
      client: "zed",
      mode: "inherit",
    });
    expect(inherit.clientDiscovery).toEqual({});
  });

  it("test07_a_write_that_changed_nothing_leaves_the_registry_alone", () => {
    const outcome = {
      dryRun: false,
      plan: { summary: "", steps: [], warnings: [], undo: "" },
      result: { applied: false, changed: [], undo: "", backups: [] },
      accessSet: { id: "work", name: "Work" },
    } as WriteOutcome;
    expect(changeOf("client-scope", outcome)).toBeNull();
  });

  it("test08_a_removed_route_reads_back_as_no_set", () => {
    const outcome = {
      dryRun: false,
      plan: { summary: "", steps: [], warnings: [], undo: "" },
      result: { applied: true, changed: ["registry"], undo: "", backups: [] },
      path: "/w/a",
      accessSet: { name: "Work" },
    } as unknown as WriteOutcome;
    expect(changeOf("folder-route", outcome)).toEqual({
      kind: "folder-route",
      path: "/w/a",
      setId: null,
    });
  });
});
