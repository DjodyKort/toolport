import { describe, expect, it } from "vitest";
import commandsGolden from "../../../plus/fixtures/ctl-envelopes/commands.json";
import type {
  CommandRow,
  CommandsData,
  DoctorCheck,
  McpDoctorData,
} from "../bridge/data";
import {
  accessSetsLine,
  canRunCommand,
  leadFor,
  selfMcpView,
  toolEntries,
} from "./model";
import { NO_SECTION, type Overview } from "./useCtl";

const commands = (commandsGolden.envelope.data as unknown as CommandsData).commands;
const row = (id: string) => commands.find((c) => c.id === id) as CommandRow;

function overview(parts: {
  checks?: DoctorCheck[];
  readable?: boolean;
  state?: McpDoctorData["state"];
}): Overview {
  return {
    ...{ status: NO_SECTION, doctor: NO_SECTION, mcp: NO_SECTION, tools: NO_SECTION },
    commands: NO_SECTION,
    status: {
      ...NO_SECTION,
      data: {
        version: "2.3.6",
        dataDir: "/data",
        serverCount: 3,
        accessSetCount: 2,
        defaultAccess: "local",
        secretsBackend: "encrypted-file",
        gateway: { path: "/data/bin/gw", present: true },
        registry: {
          path: "/data/registry.json",
          exists: true,
          readable: parts.readable ?? true,
          error: parts.readable === false ? "bad json" : null,
        },
      },
    },
    doctor: {
      ...NO_SECTION,
      data: { healthy: true, checks: parts.checks ?? [] },
    },
    mcp: {
      ...NO_SECTION,
      data: {
        state: parts.state ?? "enabled",
        switchedOn: true,
        defaultAccess: null,
        clientAccessSets: [],
        checks: [],
      },
    },
  } as Overview;
}

describe("leadFor", () => {
  it("test01_an_unreadable_registry_outranks_every_other_message", () => {
    const lead = leadFor(
      overview({
        readable: false,
        checks: [{ name: "plusBuilt", status: "warn", detail: "x" }],
      }),
    );
    expect(lead).toMatchObject({ variant: "danger", detail: "bad json" });
  });

  it("test02_a_failed_check_names_the_checks_in_plain_words", () => {
    const lead = leadFor(
      overview({ checks: [{ name: "secretsBackend", status: "fail", detail: "x" }] }),
    );
    expect(lead.title).toBe("1 installation check failed");
    expect(lead.detail).toBe("Secrets storage");
  });

  it("test03_the_fork_seam_warning_leads_the_page", () => {
    const lead = leadFor(
      overview({
        checks: [{ name: "plusBuilt", status: "warn", detail: "no seam here" }],
      }),
    );
    expect(lead.variant).toBe("warning");
    expect(lead.detail).toContain("without the fork seam");
  });

  it("test04_a_healthy_install_without_self_management_is_info_not_a_problem", () => {
    expect(leadFor(overview({ state: "missing" })).variant).toBe("info");
    expect(leadFor(overview({ state: "enabled" })).variant).toBe("success");
  });
});

describe("selfMcpView", () => {
  it("test01_only_a_registered_server_can_be_removed", () => {
    expect(selfMcpView("missing").removable).toBe(false);
    expect(selfMcpView("opted-out").removable).toBe(false);
    for (const state of ["enabled", "disabled", "not-enabled"] as const) {
      expect(selfMcpView(state).removable).toBe(true);
    }
  });

  it("test02_a_server_the_user_switched_off_is_not_offered_a_reconnect", () => {
    expect(selfMcpView("disabled").primary).toBe("open-servers");
  });
});

describe("accessSetsLine", () => {
  it("test01_says_every_enabled_server_when_no_default_set_is_chosen", () => {
    expect(
      accessSetsLine({
        state: "enabled",
        switchedOn: true,
        defaultAccess: null,
        clientAccessSets: [{ id: "focus", member: false, optedOut: true }],
        checks: [],
      }),
    ).toBe("Default access: every enabled server · focus: taken out on purpose");
  });
});

describe("canRunCommand", () => {
  it("test01_runs_read_only_commands_without_operands", () => {
    for (const id of ["status", "doctor", "commands", "mcp doctor", "mcp tools"]) {
      expect(canRunCommand(row(id)), id).toBe(true);
    }
  });

  it("test02_never_runs_commands_that_change_things_or_need_an_operand", () => {
    for (const id of ["mcp install", "mcp uninstall", "mcp call"]) {
      expect(canRunCommand(row(id)), id).toBe(false);
    }
    expect(canRunCommand({ ...row("status"), surface: "terminal" })).toBe(false);
    expect(canRunCommand({ ...row("status"), needs: ["browser"] })).toBe(false);
    expect(canRunCommand({ ...row("status"), planned: true })).toBe(false);
  });
});

describe("toolEntries", () => {
  it("test01_a_tool_that_asks_for_confirmation_is_gated_even_when_the_registry_says_read", () => {
    const [entry] = toolEntries(
      [{ name: "x", tier: "read", toolTier: 1, command: null, dryRun: "none" }],
      [
        {
          name: "x",
          description: "d",
          tier: 3,
          gate: "always",
          dryRunDefault: false,
          params: [{ name: "dry_run" }],
        },
      ],
    );
    expect(entry).toMatchObject({ gated: true, previews: true });
  });
});
