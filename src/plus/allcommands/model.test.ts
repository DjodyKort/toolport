import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { CommandRow, CommandsData } from "../bridge/data";
import { commandsFixture } from "../fixtures/commandsRegistry";
import {
  baseArgv,
  commandLine,
  commandRows,
  effectiveTier,
  emptyValues,
  formFlags,
  groupCounts,
  hasMcpCall,
  matchesQuery,
  parseToolArgs,
  phraseFor,
  planRun,
  planTool,
  problems,
  shellQuote,
  splitWords,
  stdinNeed,
  toolOnlyRows,
  type FormValues,
} from "./model";

const row = (id: string): CommandRow => {
  const found = commandsFixture.commands.find((candidate) => candidate.id === id);
  if (!found) throw new Error(`no fixture row ${id}`);
  return found;
};

const filled = (patch: Partial<FormValues>): FormValues => ({
  ...emptyValues(),
  ...patch,
});

describe("argv of a command", () => {
  it("is the path, the operands and the flags as --flag=value", () => {
    const values = filled({
      operands: { profile: "work" },
      flags: { "--name": "Team", "--add-server": "a, b" },
    });
    expect(baseArgv(row("profile edit"), values)).toEqual([
      "profile",
      "edit",
      "work",
      "--name=Team",
      "--add-server=a, b",
    ]);
  });

  it("skips empty values, sets a bool only when on, and repeats a repeatable flag", () => {
    const values = filled({
      operands: { name: "  " },
      flags: {
        "--command": "",
        "--url": " https://example.test/mcp ",
        "--env": "A=1\n\n B=2 \n",
        "--transport": "http",
      },
    });
    expect(baseArgv(row("server new"), values)).toEqual([
      "server",
      "new",
      "--url=https://example.test/mcp",
      "--env=A=1",
      "--env=B=2",
      "--transport=http",
    ]);
    expect(
      baseArgv(row("skills sync"), filled({ flags: { "--project": true } })),
    ).toEqual(["skills", "sync", "--project"]);
    expect(
      baseArgv(row("skills sync"), filled({ flags: { "--project": false } })),
    ).toEqual(["skills", "sync"]);
  });

  it("splits a variadic operand into words and keeps quoted words together", () => {
    expect(splitWords(`one "two words" 'three four' five`)).toEqual([
      "one",
      "two words",
      "three four",
      "five",
    ]);
  });

  it("never offers or sends a hidden flag, a sensitive flag or the preview flag", () => {
    const names = formFlags(row("profile edit")).map((flag) => flag.name);
    expect(names).not.toContain("--force");
    expect(names).not.toContain("--dry-run");
    expect(formFlags(row("secret set"))).toEqual([]);
    const sneaky = filled({
      operands: { profile: "work" },
      flags: { "--force": true, "--dry-run": true },
    });
    expect(baseArgv(row("profile edit"), sneaky)).toEqual(["profile", "edit", "work"]);
  });

  it("adds --passphrase-stdin by itself, and only when a secret is waiting", () => {
    expect(baseArgv(row("sync init"), emptyValues())).toEqual(["sync", "init"]);
    expect(baseArgv(row("sync init"), filled({ secretFilled: true }))).toEqual([
      "sync",
      "init",
      "--passphrase-stdin",
    ]);
    expect(
      baseArgv(
        row("secret set"),
        filled({ operands: { server: "s", key: "K" }, secretFilled: true }),
      ),
    ).toEqual(["secret", "set", "s", "K"]);
  });
});

describe("what the form still lacks", () => {
  it("asks for required operands and a value on stdin", () => {
    expect(problems(row("secret set"), emptyValues())).toEqual([
      "server is required",
      "key is required",
      "The value on stdin is required",
    ]);
    const ready = filled({ operands: { server: "s", key: "K" }, secretFilled: true });
    expect(problems(row("secret set"), ready)).toEqual([]);
  });

  it("does not ask for stdin when the input is only an optional payload", () => {
    const checkpoint = commandsFixture.commands.find((r) => r.id === "attention ls")!;
    expect(stdinNeed(checkpoint)).toBeNull();
    const payload: CommandRow = { ...checkpoint, needs: ["stdin"], flags: [] };
    expect(stdinNeed(payload)).toBe("payload");
    expect(problems(payload, emptyValues())).toEqual([]);
  });

  it("wants one of a oneOf group, and the passphrase on stdin satisfies its own flag", () => {
    const new_ = row("server new");
    expect(problems(new_, filled({ operands: { name: "x" } }))).toEqual([
      "Give at least one of --command, --url",
    ]);
    expect(
      problems(new_, filled({ operands: { name: "x" }, flags: { "--url": "u" } })),
    ).toEqual([]);
    expect(problems(row("sync init"), emptyValues())).toEqual([
      "The value on stdin is required",
      "Give at least one of --passphrase-stdin, --passphrase-env",
    ]);
    expect(problems(row("sync init"), filled({ secretFilled: true }))).toEqual([]);
  });

  it("refuses an operand that starts with a dash (it would be read as a flag)", () => {
    expect(
      problems(row("server uninstall"), filled({ operands: { server: "--home=/x" } })),
    ).toEqual(["server cannot start with a dash"]);
    const search: CommandRow = {
      ...row("server ls"),
      operands: [{ name: "query", required: false, variadic: true }],
    };
    expect(problems(search, filled({ operands: { query: "ok --bad" } }))).toEqual([
      "query cannot start with a dash",
    ]);
  });

  it("checks a whole-number flag", () => {
    const limit: CommandRow = {
      ...row("server ls"),
      flags: [{ ...row("server new").flags[0], name: "--limit", valueType: "integer" }],
    };
    expect(problems(limit, filled({ flags: { "--limit": "x" } }))).toEqual([
      "--limit must be a whole number",
    ]);
    expect(problems(limit, filled({ flags: { "--limit": "12" } }))).toEqual([]);
  });

  it("asks for a required flag", () => {
    const needsName: CommandRow = {
      ...row("server ls"),
      flags: [{ ...row("server new").flags[0], name: "--name", required: true }],
    };
    expect(problems(needsName, emptyValues())).toEqual(["--name is required"]);
  });
});

describe("effective tier and the plan of a run", () => {
  it("stays a read until an escalating flag is set, then previews", () => {
    const imp = row("client import");
    const bare = filled({ operands: { client: "acme" } });
    expect(imp).toMatchObject({ baseTier: "read", tier: "write" });
    const readOnly = imp;
    expect(effectiveTier(readOnly, bare)).toBe("read");
    expect(planRun(readOnly, bare)).toEqual({
      kind: "run",
      argv: ["client", "import", "acme"],
      ask: false,
    });
    const selected = filled({ operands: { client: "acme" }, flags: { "--select": "a" } });
    expect(effectiveTier(readOnly, selected)).toBe("write");
    expect(planRun(readOnly, selected)).toEqual({
      kind: "preview",
      previewArgv: ["client", "import", "acme", "--select=a", "--dry-run"],
      applyArgv: ["client", "import", "acme", "--select=a"],
      tier: "write",
    });
  });

  it("lets an operand escalate when the row says so", () => {
    const pin: CommandRow = {
      ...row("client import"),
      baseTier: "read",
      tier: "write",
      operandEscalates: true,
      flags: [],
      operands: [{ name: "version", required: false, variadic: false }],
    };
    expect(effectiveTier(pin, emptyValues())).toBe("read");
    expect(effectiveTier(pin, filled({ operands: { version: "1.2" } }))).toBe("write");
  });

  it("previews a destructive command with its flag and applies without it", () => {
    const values = filled({
      operands: { server: "acme-erp" },
      flags: { "--keep-secrets": true },
    });
    expect(planRun(row("server uninstall"), values)).toEqual({
      kind: "preview",
      previewArgv: ["server", "uninstall", "acme-erp", "--keep-secrets", "--dry-run"],
      applyArgv: ["server", "uninstall", "acme-erp", "--keep-secrets"],
      tier: "destructive",
    });
  });

  it("previews an unless-applied command without its flag and applies with it", () => {
    expect(planRun(row("compression update"), emptyValues())).toEqual({
      kind: "preview",
      previewArgv: ["compression", "update"],
      applyArgv: ["compression", "update", "--accept"],
      tier: "write",
    });
    expect(formFlags(row("compression update"))).toEqual([]);
  });

  it("runs a plain read, and asks first when the read costs something", () => {
    expect(planRun(row("status"), emptyValues())).toEqual({
      kind: "run",
      argv: ["status"],
      ask: false,
    });
    expect(
      planRun(row("council ask"), filled({ operands: { question: "why" } })),
    ).toEqual({
      kind: "run",
      argv: ["council", "ask", "why"],
      ask: true,
    });
  });

  it("confirms a writer that has no preview, and never previews one that reads stdin", () => {
    const set = filled({ operands: { server: "s", key: "K" }, secretFilled: true });
    expect(planRun(row("secret set"), set)).toEqual({
      kind: "direct",
      argv: ["secret", "set", "s", "K"],
      tier: "write",
    });
    const previewing: CommandRow = {
      ...row("secret set"),
      preview: { mode: "flag", flag: "--dry-run" },
    };
    expect(planRun(previewing, set).kind).toBe("direct");
    expect(
      planRun(row("sync rotate-passphrase"), filled({ secretFilled: true })),
    ).toMatchObject({
      kind: "direct",
      tier: "destructive",
    });
  });

  it("never runs a terminal-only command", () => {
    expect(planRun(row("compression run"), emptyValues())).toEqual({
      kind: "terminal",
      argv: ["compression", "run"],
    });
  });

  it("types the first required operand when it is short, else the command", () => {
    const server = row("server uninstall");
    expect(phraseFor(server, filled({ operands: { server: "acme-erp" } }))).toBe(
      "acme-erp",
    );
    expect(phraseFor(server, emptyValues())).toBe("server uninstall");
    expect(phraseFor(server, filled({ operands: { server: "has a space" } }))).toBe(
      "server uninstall",
    );
    expect(phraseFor(server, filled({ operands: { server: "x".repeat(60) } }))).toBe(
      "server uninstall",
    );
  });
});

describe("the command line", () => {
  it("quotes what a shell would split and leaves plain words alone", () => {
    expect(shellQuote("--name=Team")).toBe("--name=Team");
    expect(shellQuote("a b")).toBe("'a b'");
    expect(shellQuote("it's")).toBe(`'it'\\''s'`);
    expect(shellQuote("$(rm)")).toBe("'$(rm)'");
    expect(commandLine(["profile", "edit", "my work", "--name=A&B"])).toBe(
      "toolportctl profile edit 'my work' '--name=A&B'",
    );
  });
});

describe("running a tool", () => {
  const tool = (patch: Partial<CommandsData["tools"][number]>) => ({
    name: "t",
    tier: "write" as const,
    toolTier: 3,
    dryRun: "none" as const,
    command: null,
    ...patch,
  });

  it("runs a read as it is", () => {
    expect(planTool(tool({ tier: "read", toolTier: 1 }), { a: 1 })).toEqual({
      kind: "run",
      stdin: '{"a":1}',
    });
  });

  it("previews with dry_run and applies with dry_run false and confirm for tier 3 and up", () => {
    expect(planTool(tool({ dryRun: "default-on" }), { a: 1 })).toEqual({
      kind: "preview",
      previewStdin: '{"a":1,"dry_run":true}',
      applyStdin: '{"a":1,"dry_run":false,"confirm":true}',
      tier: "write",
    });
    expect(planTool(tool({ dryRun: "param", toolTier: 2 }), {})).toMatchObject({
      applyStdin: '{"dry_run":false}',
    });
  });

  it("confirms a writer without a dry run and sends confirm for tier 3 and up", () => {
    expect(planTool(tool({}), { body: "x" })).toEqual({
      kind: "direct",
      stdin: '{"body":"x","confirm":true}',
      tier: "write",
    });
    expect(planTool(tool({ tier: "destructive", toolTier: 4 }), {})).toMatchObject({
      kind: "direct",
      tier: "destructive",
    });
  });

  it("parses the arguments as one JSON object", () => {
    expect(parseToolArgs("  ")).toEqual({});
    expect(parseToolArgs('{"a":[1]}')).toEqual({ a: [1] });
    expect(parseToolArgs("[1]")).toMatch(/one JSON object/);
    expect(parseToolArgs("null")).toMatch(/one JSON object/);
    expect(parseToolArgs("{nope")).toMatch(/not valid JSON/);
  });

  it("offers the tools no command covers and finds mcp call", () => {
    expect(toolOnlyRows(commandsFixture).map((entry) => entry.name)).toEqual([
      "skills_get",
      "skills_edit_body",
      "styles_apply_note",
      "skills_git_push",
    ]);
    expect(hasMcpCall(commandsFixture)).toBe(false);
  });
});

describe("finding a command", () => {
  const rows = commandRows(commandsFixture);

  it("matches every word against the id, the summary and the flag names", () => {
    const hit = (query: string) =>
      rows.filter((candidate) => matchesQuery(candidate, query)).map((r) => r.id);
    expect(hit("")).toHaveLength(rows.length);
    expect(hit("uninstall")).toEqual(["server uninstall"]);
    expect(hit("UNINSTALL entries")).toEqual(["server uninstall"]);
    expect(hit("--keep-secrets")).toEqual(["server uninstall"]);
    expect(hit("no such thing")).toEqual([]);
  });

  it("counts commands per group, groups sorted", () => {
    const counts = groupCounts(rows);
    expect(counts.map((entry) => entry.group)).toEqual(
      [...counts.map((entry) => entry.group)].sort((a, b) => a.localeCompare(b)),
    );
    expect(counts.find((entry) => entry.group === "server")?.count).toBe(3);
  });
});

describe("every row of the real registry", () => {
  const golden = JSON.parse(
    readFileSync(
      join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json"),
      "utf8",
    ),
  ).envelope.data as CommandsData;
  const real = commandRows(golden);

  function fillAll(command: CommandRow): FormValues {
    return {
      operands: Object.fromEntries(
        command.operands.map((operand) => [operand.name, "x"]),
      ),
      flags: Object.fromEntries(
        formFlags(command).map((flag) => [
          flag.name,
          flag.valueType === "bool" ? true : flag.choices?.[0] ? flag.choices[0] : "1",
        ]),
      ),
      secretFilled: true,
    };
  }

  it("has rows to test", () => {
    expect(real.length).toBeGreaterThan(100);
  });

  it("never sends a hidden or sensitive flag, except --passphrase-stdin", () => {
    for (const command of real) {
      const forbidden = command.flags
        .filter((flag) => flag.hidden || flag.sensitive)
        .map((flag) => flag.name)
        .filter((name) => name !== "--passphrase-stdin");
      const plan = planRun(command, fillAll(command));
      const argvs =
        plan.kind === "preview" ? [plan.previewArgv, plan.applyArgv] : [plan.argv];
      for (const argv of argvs)
        for (const name of forbidden)
          expect(
            argv.some((token) => token === name || token.startsWith(`${name}=`)),
            `${command.id} sends ${name}`,
          ).toBe(false);
    }
  });

  it("never runs a change without a preview or a confirmation", () => {
    for (const command of real) {
      for (const values of [emptyValues(), fillAll(command)]) {
        const plan = planRun(command, values);
        const tier = effectiveTier(command, values);
        if (plan.kind === "run") expect(tier, command.id).toBe("read");
        if (tier === "destructive" && plan.kind !== "terminal")
          expect(plan.kind, command.id).toMatch(/preview|direct/);
      }
    }
  });

  it("previews with the row's own flag and applies without a dry run", () => {
    for (const command of real) {
      const plan = planRun(command, fillAll(command));
      if (plan.kind !== "preview") continue;
      const flag = command.preview?.flag;
      expect(flag, command.id).toBeTruthy();
      if (command.preview?.mode === "flag") {
        expect(plan.previewArgv, command.id).toContain(flag);
        expect(plan.applyArgv, command.id).not.toContain(flag);
      } else {
        expect(plan.applyArgv, command.id).toContain(flag);
        expect(plan.previewArgv, command.id).not.toContain(flag);
      }
    }
  });

  it("shows a terminal command's line and offers no run", () => {
    const terminal = real.filter(
      (command) =>
        command.surface === "terminal" || command.needs.includes("terminal-only"),
    );
    expect(terminal.length).toBeGreaterThan(0);
    for (const command of terminal) {
      const plan = planRun(command, fillAll(command));
      if (plan.kind !== "terminal") throw new Error(`${command.id} is ${plan.kind}`);
      expect(commandLine(plan.argv)).toMatch(/^toolportctl /);
    }
  });

  it("can be filled in so that nothing is missing", () => {
    for (const command of real) {
      expect(problems(command, fillAll(command)), command.id).toEqual([]);
    }
  });
});
