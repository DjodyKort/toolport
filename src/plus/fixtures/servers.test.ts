import { describe, expect, it } from "vitest";
import {
  clientLsData,
  clientSyncData,
  commandsData,
  doctorData,
  profileCreateData,
  profileEditData,
  profileLsData,
  profileRmData,
  serverInfoData,
  serverLsData,
  serverUninstallData,
  skillsSyncData,
  statusData,
} from "../bridge/data";
import { check, type Shape } from "../bridge/shape";
import {
  clientDirectAddData,
  clientDirectLsData,
  clientDirectRmData,
  clientEditData,
  clientImportData,
} from "../types/client";
import { inspectData } from "../types/inspect";
import { profileInspectData } from "../types/profile";
import {
  serverEditData,
  serverInstallData,
  serverNewData,
  serverSearchData,
} from "../types/server";
import { buildServerViews, gatewayServers } from "../servers/model";
import { CtlReplyFailure } from "./ctlReply";
import { serversCtlFixtures, serversWorld } from "./servers";

const SHAPES: Array<[RegExp, Shape<unknown>]> = [
  [/^commands$/, commandsData],
  [/^doctor$/, doctorData],
  [/^server ls$/, serverLsData],
  [/^server info /, serverInfoData],
  [/^server search /, serverSearchData],
  [/^server uninstall /, serverUninstallData],
  [/^inspect /, inspectData],
  [/^profile ls$/, profileLsData],
  [/^profile inspect /, profileInspectData],
  [/^profile create /, profileCreateData],
  [/^profile edit /, profileEditData],
  [/^profile rm /, profileRmData],
  [/^client ls$/, clientLsData],
  [/^client sync/, clientSyncData],
  [/^client edit /, clientEditData],
  [/^client import /, clientImportData],
  [/^client direct ls$/, clientDirectLsData],
  [/^client direct add /, clientDirectAddData],
  [/^client direct rm /, clientDirectRmData],
  [/^server install /, serverInstallData],
  [/^server new /, serverNewData],
  [/^server edit /, serverEditData],
  [/^skills sync/, skillsSyncData],
];

describe("the Servers screen fixtures", () => {
  it("have the shape of the real toolportctl output, for every key", () => {
    const unchecked: string[] = [];
    for (const [key, reply] of serversCtlFixtures) {
      if (reply instanceof CtlReplyFailure) continue;
      if (key === "status") {
        const { directEntries, ...rest } = reply as Record<string, unknown>;
        expect(typeof directEntries).toBe("number");
        expect(check(statusData, rest), key).toEqual([]);
        continue;
      }
      const shape = SHAPES.find(([pattern]) => pattern.test(key))?.[1];
      if (!shape) {
        unchecked.push(key);
        continue;
      }
      expect(check(shape, reply), key).toEqual([]);
    }
    expect(unchecked, "fixture keys with no shape to check them against").toEqual([]);
  });

  it("describe one world: every id points at a server, profile or client that exists", () => {
    const servers = new Set(serversWorld.serverLs.servers.map((server) => server.id));
    const profiles = new Set(
      serversWorld.profileLs.profiles.map((profile) => profile.id),
    );
    for (const profile of serversWorld.profileLs.profiles) {
      for (const member of profile.servers)
        expect(servers, member.id).toContain(member.id);
    }
    for (const client of serversWorld.clientLs.clients) {
      if (client.scope) expect(profiles, client.id).toContain(client.scope);
    }
    const names = new Set(serversWorld.serverLs.servers.map((server) => server.name));
    for (const row of serversWorld.status.auth.servers) {
      expect(servers.has(row.server) || names.has(row.server), row.server).toBe(true);
    }
    for (const id of gatewayServers(serversWorld.status).keys())
      expect(servers).toContain(id);
    for (const server of servers) {
      expect(serversCtlFixtures.has(`server info ${server}`), server).toBe(true);
      expect(serversCtlFixtures.has(`inspect ${server}`), server).toBe(true);
    }
  });

  it("hold the cases the screen has to tell apart", () => {
    const { serverLs, profileLs, status } = serversWorld;
    const states = buildServerViews(serverLs, profileLs, status).map(
      (view) => view.state,
    );
    expect([...new Set(states)].sort()).toEqual([
      "connected",
      "disabled",
      "failed",
      "login",
    ]);
    const failing = serversCtlFixtures.get("inspect srv-issues");
    expect(failing).toBeInstanceOf(CtlReplyFailure);
    expect((failing as CtlReplyFailure).message).toMatch(/401/);
  });

  it("never carry the value of a secret: environment rows are a key and a flag", () => {
    for (const [key, reply] of serversCtlFixtures) {
      if (!key.startsWith("server info ")) continue;
      const env = (reply as { env: Array<Record<string, unknown>> }).env;
      for (const row of env)
        expect(Object.keys(row).sort(), key).toEqual(["key", "secret"]);
    }
  });
});
