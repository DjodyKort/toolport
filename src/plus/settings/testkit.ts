import { vi } from "vitest";
import type { Registry } from "@/lib/types";

/** The registry the page reads access sets from. */
export function registryWithSets(...ids: string[]): Registry {
  return {
    version: 3,
    servers: [],
    profiles: ids.map((id) => ({ id, name: id, enabledServerIds: [] })),
    activeProfileId: ids[0] ?? null,
    defaultAccessContextId: ids[0] ?? null,
    clientScopes: {},
  } as unknown as Registry;
}

export const getRegistryMock = vi.fn();
