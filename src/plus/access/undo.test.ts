import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { undoSteps } from "./undo";

const dir = join(__dirname, "../../../plus/fixtures/ctl-envelopes");

describe("the undo lines of the access writers", () => {
  it("test01_a_quoted_name_stays_one_word", () => {
    expect(undoSteps("toolportctl access-set rm 'Deep Work'")).toEqual([
      { kind: "ctl", argv: ["access-set", "rm", "Deep Work"] },
    ]);
  });

  it("test02_an_apostrophe_inside_a_quoted_name_is_kept", () => {
    expect(undoSteps(`toolportctl access-set rm 'Ann'\\''s set'`)).toEqual([
      { kind: "ctl", argv: ["access-set", "rm", "Ann's set"] },
    ]);
  });

  it("test03_a_chain_of_commands_runs_in_order", () => {
    expect(
      undoSteps(
        "toolportctl access-set add Bare --server srv-alpha && toolportctl client scope claude-code Bare && toolportctl folder route add /work/proj Bare",
      ),
    ).toEqual([
      { kind: "ctl", argv: ["access-set", "add", "Bare", "--server", "srv-alpha"] },
      { kind: "ctl", argv: ["client", "scope", "claude-code", "Bare"] },
      { kind: "ctl", argv: ["folder", "route", "add", "/work/proj", "Bare"] },
    ]);
  });

  it("test04_the_discovery_tool_undoes_through_its_arguments", () => {
    expect(undoSteps("client_discovery_set client=claude-code mode=inherit")).toEqual([
      {
        kind: "tool",
        tool: "client_discovery_set",
        args: { client: "claude-code", mode: "inherit" },
      },
    ]);
  });

  it("test05_a_line_that_is_not_a_command_is_refused_instead_of_guessed", () => {
    expect(() => undoSteps("do something else entirely")).toThrow(/Cannot undo/);
  });

  it("test06_every_undo_line_of_the_goldens_parses_into_commands_of_the_cli", () => {
    const files = readdirSync(dir).filter((file) =>
      /^(access-set|client-scope|folder-route)-.*\.json$/.test(file),
    );
    let seen = 0;
    for (const file of files) {
      const golden = JSON.parse(readFileSync(join(dir, file), "utf8")) as {
        envelope: { data?: { plan?: { undo?: string } } };
      };
      const undo = golden.envelope.data?.plan?.undo;
      if (!undo) continue;
      seen++;
      for (const step of undoSteps(undo)) {
        expect(step.kind).toBe("ctl");
        if (step.kind === "ctl") {
          expect(["access-set", "client", "folder"], file).toContain(step.argv[0]);
        }
      }
    }
    expect(seen).toBeGreaterThan(8);
  });
});
