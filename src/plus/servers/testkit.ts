import { vi } from "vitest";
import type { Registry, ServerEntry } from "@/lib/types";
import { stdioServer, type WorldServer } from "../fixtures/serverWorld";

/** Every `invoke` the fixture saw, so a test can read the argv a click ran. */
export const invokeLog: Array<{ command: string; args: unknown }> = [];

export function ranArgv(): string[][] {
  return invokeLog
    .filter((entry) => entry.command === "plus_ctl")
    .map((entry) => (entry.args as { argv: string[] }).argv);
}

export const getRegistryMock = vi.fn();

export function worldServer(id: string, name: string, extra: Partial<WorldServer> = {}) {
  return stdioServer(id, name, extra);
}

/** The registry entry the app holds for a fixture server. */
export function entryOf(world: WorldServer): ServerEntry {
  return {
    id: world.id,
    name: world.name,
    enabled: world.enabled,
    transport: world.transport,
    command: world.command,
    args: world.args,
    url: world.url,
    cwd: world.cwd,
    source: world.source,
    inheritEnv: world.inheritEnv,
    env: world.env.map((e) => ({
      key: e.key,
      value: e.secret ? null : "x",
      secret: e.secret,
    })),
  };
}

export interface RegistryOptions {
  scopes?: Record<string, string>;
  defaultAccess?: string | null;
}

export function registryOf(
  servers: WorldServer[],
  sets: Array<{ id: string; name: string; serverIds: string[] }>,
  options: RegistryOptions = {},
): Registry {
  return {
    version: 3,
    servers: servers.map(entryOf),
    profiles: sets.map((s) => ({
      id: s.id,
      name: s.name,
      enabledServerIds: s.serverIds,
    })),
    activeProfileId: sets[0]?.id ?? null,
    defaultAccessContextId: options.defaultAccess ?? null,
    clientScopes: options.scopes ?? {},
  } as unknown as Registry;
}
