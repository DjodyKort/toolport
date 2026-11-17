import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type {
  CommandsData,
  DoctorData,
  McpDoctorData,
  McpInstallData,
  McpToolsData,
  StatusData,
} from "./data";

const dir = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

interface Golden {
  argv: string[];
  envelope: { ok: boolean; data?: unknown };
  exitCode: number;
}

function goldens(): Golden[] {
  return readdirSync(dir)
    .filter((file) => file.endsWith(".json"))
    .map((file) => JSON.parse(readFileSync(join(dir, file), "utf8")) as Golden);
}

const keys = (value: unknown) => Object.keys(value as object).sort();

describe("the envelope types match the Rust goldens", () => {
  it("test01_status_has_the_fields_the_page_reads", () => {
    const all = goldens().filter((g) => g.argv.join(" ") === "status");
    expect(all.length).toBeGreaterThan(1);
    for (const { envelope } of all) {
      const data = envelope.data as StatusData;
      expect(keys(data)).toEqual(
        [
          "accessSetCount",
          "auth",
          "dataDir",
          "defaultAccess",
          "gateway",
          "registry",
          "secretsBackend",
          "serverCount",
          "version",
        ].sort(),
      );
      expect(keys(data.registry)).toEqual(["error", "exists", "path", "readable"]);
    }
  });

  it("test02_doctor_checks_carry_name_status_and_detail_whether_healthy_or_not", () => {
    const all = goldens().filter((g) => g.argv.join(" ") === "doctor");
    expect(all.length).toBeGreaterThan(2);
    for (const { envelope } of all) {
      const data = envelope.data as DoctorData;
      expect(data.checks.length).toBeGreaterThan(0);
      for (const check of data.checks) {
        expect(keys(check)).toEqual(["detail", "name", "status"]);
        expect(["ok", "warn", "fail"]).toContain(check.status);
      }
    }
  });

  it("test03_mcp_doctor_reports_a_state_the_page_knows", () => {
    const all = goldens().filter((g) => g.argv.join(" ") === "mcp doctor");
    const states = new Set<string>();
    for (const { envelope } of all) {
      const data = envelope.data as McpDoctorData;
      states.add(data.state);
      expect(keys(data)).toEqual(
        ["checks", "clientAccessSets", "defaultAccess", "state", "switchedOn"].sort(),
      );
    }
    for (const state of states) {
      expect(["missing", "opted-out", "not-enabled", "enabled", "disabled"]).toContain(
        state,
      );
    }
  });

  it("test04_mcp_install_answers_with_the_access_set_it_listed", () => {
    const all = goldens().filter(
      (g) => g.argv.slice(0, 2).join(" ") === "mcp install" && g.envelope.ok,
    );
    expect(all.length).toBeGreaterThan(1);
    for (const { envelope } of all) {
      expect(keys(envelope.data as McpInstallData)).toEqual(
        ["accessSet", "action", "command", "enabled", "id"].sort(),
      );
    }
  });

  it("test05_commands_and_tools_listings_have_the_fields_of_the_all_commands_page", () => {
    const registry = goldens().find((g) => g.argv.join(" ") === "commands")!.envelope
      .data as CommandsData;
    for (const command of registry.commands) {
      for (const key of [
        "id",
        "kind",
        "path",
        "summary",
        "tier",
        "surface",
        "needs",
        "flags",
        "operands",
        "oneOf",
        "planned",
      ]) {
        expect(command, `${command.id}.${key}`).toHaveProperty(key);
      }
    }
    expect(keys(registry.tools[0])).toEqual(
      ["command", "dryRun", "name", "tier", "toolTier"].sort(),
    );
    const tools = goldens().find((g) => g.argv.join(" ") === "mcp tools")!.envelope
      .data as McpToolsData;
    expect(tools.tools.map((t) => t.name)).toEqual(registry.tools.map((t) => t.name));
    for (const tool of tools.tools) {
      expect(keys(tool)).toEqual(
        ["description", "dryRunDefault", "gate", "name", "params", "tier"].sort(),
      );
    }
  });
});
