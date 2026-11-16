import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createServerWorld } from "../fixtures/serverWorld";
import type {
  AccessSetsData,
  MembershipData,
  RestartData,
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
        "name",
        "source",
        "transport",
        "url",
      ].sort(),
    );
    expect(keys(data.env[0])).toEqual(["key", "secret"]);
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
