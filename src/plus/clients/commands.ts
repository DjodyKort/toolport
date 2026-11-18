import { ctlData } from "../bridge/ctl";
import type { DirectListData, DirectOutcome } from "./data";

export interface DirectTarget {
  /** The server's id. */
  server: string;
  client: string;
  /** Replace or remove an entry that was edited since Toolport wrote it. */
  force?: boolean;
}

export const listDirect = (client: string) =>
  ctlData<DirectListData>(["client", "direct", "ls", "--client", client]);

function writeArgv(verb: "add" | "rm", target: DirectTarget): string[] {
  const argv = ["client", "direct", verb, target.server, "--client", target.client];
  return target.force ? [...argv, "--force"] : argv;
}

/** What the write would do; changes nothing. */
export const planDirect = (verb: "add" | "rm", target: DirectTarget) =>
  ctlData<DirectOutcome>([...writeArgv(verb, target), "--dry-run"]);

/** The write, refused when the client file or the server differs from the plan the hash names. */
export const applyDirect = (verb: "add" | "rm", target: DirectTarget, planHash: string) =>
  ctlData<DirectOutcome>([...writeArgv(verb, target), "--plan-hash", planHash]);
