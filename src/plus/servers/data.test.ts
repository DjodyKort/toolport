import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createServerWorld } from "../fixtures/serverWorld";
import { stdioServer } from "../fixtures/serverWorld";
import type {
  AccessSetsData,
  LoginData,
  MembershipData,
  ProbeData,
  ReachData,
  RestartData,
  SecretGetData,
  SecretRemoveData,
  SecretSetData,
  ServerInfoData,
  ServerListData,
  UninstallData,
} from "./data";

const root = join(__dirname, "../../../plus/fixtures");

interface Golden {
  argv: string[];
  envelope: { ok: boolean; data?: unknown };
}

function ctl(prefix: string): Golden[] {
  const dir = join(root, "ctl-envelopes");
  return readdirSync(dir)
    .filter((file) => file.startsWith(prefix) && file.endsWith(".json"))
    .map((file) => JSON.parse(readFileSync(join(dir, file), "utf8")) as Golden);
}

function selfmcp(name: string) {
  return JSON.parse(
    readFileSync(join(root, "selfmcp-envelopes", `${name}.json`), "utf8"),
  ) as {
    result: unknown;
  };
}

const keys = (value: unknown) => Object.keys(value as object).sort();

describe("the server envelope types match the Rust goldens", () => {
  it("test01_server_ls_has_the_fields_the_list_line_reads", () => {
    const [golden] = ctl("server-ls");
    const data = golden.envelope.data as ServerListData;
    expect(keys(data)).toEqual(["defaultAccess", "servers"]);
    expect(keys(data.servers[0])).toEqual(["enabled", "id", "name", "transport"]);
  });

  it("test02_server_info_has_the_fields_the_runtime_row_reads", () => {
    const [golden] = ctl("server-info");
    const data = golden.envelope.data as ServerInfoData;
    expect(keys(data)).toEqual(
      [
        "accessSets",
        "args",
        "command",
        "cwd",
        "declareClientCapabilities",
        "disabledTools",
        "enabled",
        "env",
        "forwardInstructions",
        "id",
        "inheritEnv",
        "interop",
        "maxRequestTimeoutMs",
        "mode",
        "name",
        "requestTimeoutMs",
        "source",
        "transport",
        "url",
      ].sort(),
    );
    expect(keys(data.env[0])).toEqual(["key", "secret"]);
    expect(keys(data.interop)).toEqual(
      [
        "bridgeBaseMs",
        "bridgeCeilingMs",
        "bridgeGraceMs",
        "bridgeSetBy",
        "defaultLimitMs",
        "defaultRequestTimeoutMs",
        "otherLongest",
        "pendingRedeclare",
      ].sort(),
    );
  });

  it("test03_server_restart_answers_have_a_status_and_a_row_per_gateway", () => {
    for (const { envelope } of ctl("server-restart")) {
      const data = envelope.data as RestartData;
      expect(keys(data)).toEqual(["gateways", "id", "name", "status"]);
      expect(["not_running", "dry_run"]).toContain(data.status);
    }
  });

  it("test04_server_uninstall_preview_names_clients_and_vault_entries", () => {
    const [golden] = ctl("server-uninstall");
    const data = golden.envelope.data as UninstallData;
    expect(keys(data)).toEqual(["clients", "dryRun", "id", "name", "secretsRemoved"]);
  });

  it("test05_access_sets_and_membership_results_match_the_self_management_goldens", () => {
    const sets = selfmcp("servers_list_profiles").result as AccessSetsData;
    expect(keys(sets)).toEqual(["accessSets", "defaultAccess"]);
    expect(keys(sets.accessSets[0])).toEqual(["id", "name", "serverIds"]);
    const member = selfmcp("servers_add_profile_tag.apply").result as MembershipData;
    expect(keys(member)).toEqual(["accessSets", "enabled", "name"]);
  });

  it("test06_the_fixture_toolportctl_answers_with_the_golden_shapes", () => {
    const world = createServerWorld();
    const ask = (...argv: string[]) => world.run(["server", ...argv]).data;
    expect(keys(ask("ls"))).toEqual(keys(ctl("server-ls")[0].envelope.data));
    expect(keys(ask("info", "fixture-0"))).toEqual(
      keys(ctl("server-info")[0].envelope.data),
    );
    expect(keys(ask("restart", "fixture-0"))).toEqual(
      keys(ctl("server-restart")[0].envelope.data),
    );
    expect(keys(ask("uninstall", "fixture-0", "--dry-run"))).toEqual(
      keys(ctl("server-uninstall")[0].envelope.data),
    );
  });
});

describe("the S3 envelope types match the Rust goldens", () => {
  const goldenData = (file: string) =>
    JSON.parse(
      readFileSync(join(root, "ctl-envelopes", `${file}.json`), "utf8"),
    ) as Golden;

  it("test07_secret_answers_carry_the_server_the_key_and_the_outcome", () => {
    const get = ctl("secret-get").filter((g) => g.envelope.ok);
    expect(get.length).toBeGreaterThan(1);
    const keysOf = get.map((g) => keys(g.envelope.data as SecretGetData));
    expect(keysOf).toContainEqual(["key", "server", "set"]);
    expect(keysOf).toContainEqual(["key", "server", "set", "value"]);
    expect(keys(ctl("secret-set")[0].envelope.data as SecretSetData)).toEqual([
      "key",
      "server",
      "stored",
    ]);
    expect(
      keys(
        ctl("secret-rm").find((g) => g.envelope.ok)!.envelope.data as SecretRemoveData,
      ),
    ).toEqual(["key", "removed", "server"]);
  });

  it("test08_auth_probe_and_login_answers_have_the_fields_the_login_row_reads", () => {
    const probe = ctl("auth-probe").find((g) => g.envelope.ok)!.envelope
      .data as ProbeData;
    expect(keys(probe)).toEqual(["counts", "failures", "mode", "probes", "servers"]);
    expect(keys(probe.probes[0])).toEqual([
      "nextDueAt",
      "ran",
      "server",
      "skipped",
      "tracked",
    ]);
    expect(keys(probe.servers[0])).toEqual([
      "expiresAt",
      "fix",
      "lastProbe",
      "reason",
      "server",
      "since",
      "state",
      "ttlSecs",
    ]);
    const login = ctl("auth-login").find((g) => g.envelope.ok)!.envelope
      .data as LoginData;
    expect(keys(login)).toEqual([
      "consentUrl",
      "flow",
      "message",
      "name",
      "probe",
      "server",
      "servers",
      "signedIn",
    ]);
  });

  it("test09_the_fixture_toolportctl_answers_secrets_and_logins_with_the_golden_shapes", () => {
    const alpha = stdioServer("srv-alpha", "alpha", {
      env: [{ key: "API_TOKEN", secret: true }],
    });
    const world = createServerWorld({
      servers: [alpha],
      vault: ["srv-alpha/API_TOKEN"],
      logins: { "srv-alpha": { state: "ok", checkedAgo: 10 } },
    });
    const ask = (...argv: string[]) => world.secret(["secret", ...argv], "value\n").data;
    expect(keys(ask("get", "alpha", "API_TOKEN"))).toEqual(
      keys(goldenData("secret-get.set").envelope.data),
    );
    expect(keys(ask("get", "alpha", "API_TOKEN", "--reveal"))).toEqual(
      keys(goldenData("secret-get.reveal").envelope.data),
    );
    expect(keys(ask("set", "alpha", "API_TOKEN"))).toEqual(
      keys(goldenData("secret-set.apply").envelope.data),
    );
    expect(keys(ask("rm", "alpha", "API_TOKEN"))).toEqual(
      keys(goldenData("secret-rm.apply").envelope.data),
    );
    const probe = world.auth(["auth", "probe", "--server", "alpha", "--force"]).data;
    expect(keys(probe)).toEqual(keys(goldenData("auth-probe.apply").envelope.data));
    const login = world.auth(["auth", "login", "alpha"]).data;
    expect(keys(login)).toEqual(keys(goldenData("auth-login.signed-in").envelope.data));
  });
});

describe("the S8 envelope types match the Rust goldens", () => {
  const golden = (file: string) =>
    JSON.parse(
      readFileSync(join(root, "ctl-envelopes", `${file}.json`), "utf8"),
    ) as Golden;

  it("test10_the_fixture_toolportctl_answers_mode_and_edit_with_the_golden_shapes", () => {
    const world = createServerWorld();
    const run = (...argv: string[]) => world.run(["server", ...argv]);
    expect(keys(run("mode", "fixture-0", "search").data)).toEqual(
      keys(golden("server-mode.apply").envelope.data),
    );
    expect(
      keys(run("edit", "fixture-0", "--max-request-timeout-ms", "5400000").data),
    ).toEqual(keys(golden("server-edit.limit").envelope.data));
    expect(run("edit", "fixture-0", "--max-request-timeout-ms", "0")).toMatchObject({
      ok: false,
      error: { code: "usage" },
    });
  });

  it("test11_the_fixture_counts_the_bridge_ceiling_the_way_the_golden_does", () => {
    const alpha = golden("server-info").envelope.data as ServerInfoData;
    const world = createServerWorld({
      servers: [stdioServer("srv-alpha", "alpha", { command: "<MOCK>" })],
    });
    const info = world.run(["server", "info", "alpha"]).data as ServerInfoData;
    expect(info.interop).toEqual(alpha.interop);
  });

  it("test12_client_reach_has_the_fields_the_reach_screens_read", () => {
    const goldens = ctl("client-reach").filter((g) => g.envelope.ok);
    expect(goldens.length).toBeGreaterThan(3);
    for (const golden of goldens) {
      const data = golden.envelope.data as ReachData;
      expect(keys(data)).toEqual([
        "accessRows",
        "accessSets",
        "clients",
        "defaultAccess",
        "findings",
        "folders",
        "servers",
      ]);
      expect(keys(data.accessSets[0])).toEqual([
        "clients",
        "default",
        "folders",
        "id",
        "listed",
        "name",
        "reached",
      ]);
      expect(keys(data.servers[0])).toEqual([
        "clients",
        "default",
        "direct",
        "enabled",
        "folders",
        "id",
        "listedIn",
        "name",
        "unreached",
      ]);
      expect(keys(data.defaultAccess)).toEqual([
        "accessSet",
        "accessSetName",
        "missing",
        "servers",
      ]);
    }
    const outranks = goldens
      .flatMap((g) => (g.envelope.data as ReachData).findings)
      .find((f) => f.kind === "folder-outranks");
    expect(keys(outranks)).toEqual([
      "client",
      "folder",
      "kind",
      "server",
      "servers",
      "text",
    ]);
  });
});
