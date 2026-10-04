import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CtlJobEvent, CtlResult } from "./ctl";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

type Ctl = typeof import("./ctl");
let ctl: Ctl;
let emit: (event: CtlJobEvent) => void;

const CANARY = "CANARY-ts-bridge-5d2f";

function result(over: Partial<CtlResult> = {}): CtlResult {
  return {
    job: "job-1",
    exitCode: 0,
    signal: null,
    cancelled: false,
    envelope: { ok: true, command: "status", schemaVersion: 1, data: { serverCount: 2 } },
    parseError: null,
    stderr: [],
    truncated: false,
    ...over,
  };
}

async function failure(promise: Promise<unknown>) {
  const error: unknown = await promise.then(
    () => null,
    (e: unknown) => e,
  );
  expect(error).toBeInstanceOf(ctl.CtlError);
  return error as InstanceType<Ctl["CtlError"]>;
}

function stub(replies: Record<string, unknown>) {
  invoke.mockImplementation(async (command: string) => {
    if (!(command in replies)) throw new Error(`unexpected invoke ${command}`);
    const reply = replies[command];
    return typeof reply === "function" ? reply() : reply;
  });
}

beforeEach(async () => {
  vi.resetModules();
  invoke.mockReset();
  listen.mockReset();
  listen.mockImplementation(async (_name: string, callback: (e: unknown) => void) => {
    emit = (event) => callback({ payload: event });
    return () => {};
  });
  ctl = await import("./ctl");
});

describe("runCtl", () => {
  it("starts a job, waits for its result and passes argv untouched", async () => {
    stub({ plus_ctl: "job-1", plus_ctl_result: result() });
    const job = ctl.runCtl(["status"]);
    await expect(job.id).resolves.toBe("job-1");
    const done = await job.result;
    expect(done.envelope?.data).toEqual({ serverCount: 2 });
    expect(invoke).toHaveBeenNthCalledWith(1, "plus_ctl", {
      argv: ["status"],
      stdinSecret: null,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "plus_ctl_result", { job: "job-1" });
    expect(listen).toHaveBeenCalledWith(ctl.PLUS_CTL_EVENT, expect.any(Function));
  });

  it("streams stderr lines, including ones that arrive before the job id is known", async () => {
    let release!: (value: CtlResult) => void;
    stub({
      plus_ctl: () => {
        emit({ job: "job-1", seq: 1, kind: "stderr", line: "early" });
        return "job-1";
      },
      plus_ctl_result: () => new Promise<CtlResult>((resolve) => (release = resolve)),
    });
    const lines: string[] = [];
    const job = ctl.runCtl(["auth", "login", "figma"], {
      onStderr: (line) => lines.push(line),
    });
    await job.id;
    await vi.waitFor(() => expect(release).toBeDefined());
    emit({ job: "job-1", seq: 2, kind: "stderr", line: "open this url" });
    emit({ job: "job-2", seq: 1, kind: "stderr", line: "someone else" });
    emit({ job: "job-1", seq: 3, kind: "exit", exitCode: 0, cancelled: false });
    expect(lines).toEqual(["early", "open this url"]);
    release(result({ stderr: ["early", "open this url"] }));
    await job.result;
    emit({ job: "job-1", seq: 4, kind: "stderr", line: "after the end" });
    expect(lines).toHaveLength(2);
  });

  it("cancels through the job id", async () => {
    let release!: (value: CtlResult) => void;
    stub({
      plus_ctl: "job-7",
      plus_ctl_cancel: undefined,
      plus_ctl_result: () => new Promise<CtlResult>((resolve) => (release = resolve)),
    });
    const job = ctl.runCtl(["compression", "proxy", "up"]);
    await job.cancel();
    expect(invoke).toHaveBeenCalledWith("plus_ctl_cancel", { job: "job-7" });
    await vi.waitFor(() => expect(release).toBeDefined());
    release(result({ cancelled: true, exitCode: null, signal: 9, envelope: null }));
    const done = await job.result;
    expect(done.cancelled).toBe(true);
  });

  it("cancels when the abort signal fires", async () => {
    stub({ plus_ctl: "job-8", plus_ctl_cancel: undefined, plus_ctl_result: result() });
    const controller = new AbortController();
    const job = ctl.runCtl(["usage"], { signal: controller.signal });
    controller.abort();
    await job.result;
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("plus_ctl_cancel", { job: "job-8" }),
    );
  });

  it("refuses --home and --data-dir before any IPC call", async () => {
    for (const argv of [
      ["skills", "ls", "--home", "/x"],
      ["skills", "ls", "--home=/x"],
      ["status", "--data-dir", "/x"],
    ]) {
      await expect(ctl.runCtl(argv).result).rejects.toThrow(
        /not allowed through the bridge/,
      );
    }
    expect(invoke).not.toHaveBeenCalled();
    expect(ctl.forbiddenArg(["skills", "ls", "--repo", "/x"])).toBeNull();
  });

  it("sends a secret only in the stdin field of the start call", async () => {
    stub({ plus_ctl: "job-3", plus_ctl_result: result({ stderr: ["Stored KEY"] }) });
    await ctl.runCtl(["secret", "set", "srv", "KEY"], { stdinSecret: CANARY }).result;
    expect(invoke).toHaveBeenCalledWith("plus_ctl", {
      argv: ["secret", "set", "srv", "KEY"],
      stdinSecret: CANARY,
    });
    for (const call of invoke.mock.calls) {
      expect(JSON.stringify(call[1]).split(CANARY).length - 1).toBe(
        call[0] === "plus_ctl" ? 1 : 0,
      );
      expect(JSON.stringify(call[1].argv ?? [])).not.toContain(CANARY);
    }
  });
});

describe("ctlData", () => {
  it("returns the data of a successful envelope", async () => {
    stub({ plus_ctl: "job-1", plus_ctl_result: result() });
    await expect(ctl.ctlData(["status"])).resolves.toEqual({ serverCount: 2 });
  });

  it("throws the envelope's error and keeps the data of a failed check", async () => {
    const failed = result({
      exitCode: 1,
      envelope: {
        ok: false,
        command: "doctor",
        schemaVersion: 1,
        data: { healthy: false },
        error: { code: "unhealthy", message: "one or more checks failed" },
      },
    });
    stub({ plus_ctl: "job-1", plus_ctl_result: failed });
    const error = await failure(ctl.ctlData(["doctor"]));
    expect(error.code).toBe("unhealthy");
    expect(error.message).toBe("one or more checks failed");
    expect(error.data).toEqual({ healthy: false });
  });

  it("never echoes the secret in an error", async () => {
    const failed = result({
      exitCode: 1,
      envelope: {
        ok: false,
        command: "secret set",
        schemaVersion: 1,
        error: { code: "vault", message: "could not write [redacted]" },
      },
    });
    stub({ plus_ctl: "job-1", plus_ctl_result: failed });
    const error = await failure(
      ctl.ctlData(["secret", "set", "s", "K"], { stdinSecret: CANARY }),
    );
    expect(`${error.message} ${JSON.stringify(error.result)}`).not.toContain(CANARY);
  });

  it("reports a bridge-level failure when there is no envelope", async () => {
    stub({
      plus_ctl: "job-1",
      plus_ctl_result: result({
        exitCode: 3,
        envelope: null,
        parseError: "stdout holds more than one line",
      }),
    });
    const error = await failure(ctl.ctlData(["status"]));
    expect(error.code).toBe("bridge");
    expect(error.message).toContain("more than one line");
  });
});

describe("dryRunArgv", () => {
  it("adds the flag once", () => {
    expect(ctl.dryRunArgv(["profile", "edit", "a"])).toEqual([
      "profile",
      "edit",
      "a",
      "--dry-run",
    ]);
    expect(ctl.dryRunArgv(["profile", "edit", "a", "--dry-run"])).toHaveLength(4);
  });
});
