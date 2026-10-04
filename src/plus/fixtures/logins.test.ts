import { describe, expect, it } from "vitest";
import { serverInfoData, serverLsData, statusData } from "../bridge/data";
import { check } from "../bridge/shape";
import { authHookData, authLoginData, authProbeData } from "../types/auth";
import { secretGetData, secretRmData } from "../types/secret";
import { CtlReplyFailure, CtlReplyHeld } from "./ctlReply";
import {
  loginsAuthRows,
  loginsCtlFixtures,
  loginsHook,
  loginsServerInfos,
  loginsServerLs,
  loginsStatus,
  loginsStatusline,
} from "./logins";

const replies = new Map(loginsCtlFixtures);
const reply = (argv: string) => {
  const found = replies.get(argv);
  if (found === undefined) throw new Error(`no fixture for ${argv}`);
  return found;
};

describe("Logins & secrets fixtures", () => {
  it("have the shape of the CLI output for status, server ls and server info", () => {
    expect(check(statusData, loginsStatus)).toEqual([]);
    expect(check(serverLsData, loginsServerLs)).toEqual([]);
    for (const info of Object.values(loginsServerInfos)) {
      expect(check(serverInfoData, info), info.id).toEqual([]);
    }
  });

  it("answer the statusline and the hook in the shape of their goldens", () => {
    expect(check(authHookData, loginsStatusline)).toEqual([]);
    expect(check(authHookData, { auth: loginsHook.auth })).toEqual([]);
    expect(loginsStatusline.auth.text).toContain("srv-issues");
    expect(
      "hookSpecificOutput" in loginsHook &&
        loginsHook.hookSpecificOutput.additionalContext,
    ).toContain("srv-issues");
  });

  it("answer a sign-in and a probe in the shape of their goldens", () => {
    expect(check(authLoginData, reply("auth login srv-design") as object)).toEqual([]);
    const held = reply("auth login srv-issues") as CtlReplyHeld;
    expect(held).toBeInstanceOf(CtlReplyHeld);
    expect(check(authLoginData, held.data)).toEqual([]);
    const probe = reply("auth probe --force") as { servers: Array<{ fix: unknown }> };
    const errors = check(authProbeData, probe);
    // The golden has one broken server, so its shape asks for a `fix` object on every row. A
    // row that works has `fix: null` in the real output; those are the only differences.
    const withoutFix = errors.filter((line) => !/servers\[\d+\]\.fix/.test(line));
    expect(withoutFix).toEqual([]);
    expect(probe.servers.some((row) => row.fix === null)).toBe(true);
  });

  it("answer the secret commands in the shape of their goldens", () => {
    expect(check(secretGetData, reply("secret get srv-erp ERP_API_KEY"))).toEqual([]);
    expect(
      check(secretGetData, reply("secret get srv-erp ERP_API_KEY --reveal")),
    ).toEqual([]);
    expect(check(secretRmData, reply("secret rm srv-erp ERP_API_KEY"))).toEqual([]);
    const unset = reply("secret get srv-erp ERP_WEBHOOK_SECRET");
    expect(unset).toBeInstanceOf(CtlReplyFailure);
    expect((unset as CtlReplyFailure).code).toBe("not_found");
  });

  it("have a reply for every read the screens make, per server", () => {
    for (const server of loginsServerLs.servers) {
      for (const argv of [
        `server info ${server.id}`,
        `auth probe --server ${server.id}`,
        `auth probe --server ${server.id} --force`,
      ]) {
        expect(replies.has(argv), argv).toBe(true);
      }
    }
    for (const row of loginsAuthRows) {
      expect(loginsServerLs.servers.map((s) => s.id)).toContain(row.server);
    }
  });

  it("keep a secret value out of every reply except the reveal placeholder", () => {
    for (const [argv, data] of loginsCtlFixtures) {
      if (argv.endsWith("--reveal")) continue;
      expect(JSON.stringify(data), argv).not.toContain("fixture-vaulted-value");
    }
    const names = JSON.stringify(Object.values(loginsServerInfos));
    expect(names).toContain("ERP_API_KEY");
    expect(names).not.toMatch(/"value"/);
  });
});
