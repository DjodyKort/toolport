import { beforeEach, describe, expect, it } from "vitest";
import type { CtlResult } from "../bridge/ctl";
import { plusFixtureFromSearch, plusFixtureIpc, resetPlusFixture } from "./ipc";

function run(argv: string[], stdinSecret?: string): CtlResult {
  const job = plusFixtureIpc("plus_ctl", { argv, stdinSecret: stdinSecret ?? null });
  return plusFixtureIpc("plus_ctl_result", { job }) as CtlResult;
}

beforeEach(() => resetPlusFixture({ self: "missing" }));

describe("the in-memory toolportctl of the browser fixture", () => {
  it("test01_install_then_doctor_shows_the_server_as_enabled", () => {
    expect((run(["mcp", "doctor"]).envelope?.data as { state: string }).state).toBe(
      "missing",
    );
    const install = run(["mcp", "install", "--access-set", "work"]);
    expect(install.envelope?.data).toMatchObject({
      action: "created",
      accessSet: "work",
    });
    expect((run(["mcp", "doctor"]).envelope?.data as { state: string }).state).toBe(
      "enabled",
    );
  });

  it("test02_uninstall_twice_fails_the_second_time_like_the_real_binary", () => {
    run(["mcp", "install"]);
    expect(run(["mcp", "uninstall"]).envelope?.ok).toBe(true);
    const again = run(["mcp", "uninstall"]);
    expect(again.exitCode).toBe(1);
    expect(again.envelope?.error?.code).toBe("not_found");
  });

  it("test03_an_unhealthy_doctor_still_carries_its_checks", () => {
    resetPlusFixture({ doctor: "fail" });
    const result = run(["doctor"]);
    expect(result.exitCode).toBe(1);
    expect(result.envelope?.ok).toBe(false);
    expect(
      (result.envelope?.data as { checks: { status: string }[] }).checks.some(
        (c) => c.status === "fail",
      ),
    ).toBe(true);
  });

  it("test04_an_unknown_command_is_rejected_instead_of_answered", () => {
    const job = plusFixtureIpc("plus_ctl", { argv: ["servers", "ls"] });
    expect(() => plusFixtureIpc("plus_ctl_result", { job })).toThrow(/Unimplemented/);
  });

  it("test05_the_url_picks_the_starting_state", () => {
    plusFixtureFromSearch("?plus-self=disabled&plus-doctor=warn");
    expect((run(["mcp", "doctor"]).envelope?.data as { state: string }).state).toBe(
      "disabled",
    );
    plusFixtureFromSearch("?plus-bridge=down");
    expect(() => plusFixtureIpc("plus_ctl", { argv: ["status"] })).toThrow(
      /toolportctl not found/,
    );
  });
});
