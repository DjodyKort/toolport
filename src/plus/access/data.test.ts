import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createCtlWorld } from "../fixtures/ctlWorld";
import { stdioServer } from "../fixtures/serverWorld";

const root = join(__dirname, "../../../plus/fixtures");

interface Golden {
  argv: string[];
  envelope: { ok: boolean; data?: Record<string, unknown>; error?: { code: string } };
}

function golden(name: string): Golden {
  return JSON.parse(
    readFileSync(join(root, "ctl-envelopes", `${name}.json`), "utf8"),
  ) as Golden;
}

const keys = (value: unknown) => Object.keys(value as object).sort();

const servers = [stdioServer("srv-alpha", "alpha"), stdioServer("srv-beta", "beta")];
const sets = [
  { id: "default", name: "Default", serverIds: ["srv-alpha"] },
  { id: "bare", name: "Bare", serverIds: ["srv-alpha"] },
  { id: "wide", name: "Wide", serverIds: ["srv-alpha", "srv-beta"] },
];

function world() {
  return createCtlWorld({
    servers: { servers, sets, defaultAccess: "default" },
    reach: {
      clients: [{ id: "claude-code", set: "bare" }],
      folders: [{ path: "/work/proj", set: "bare" }],
    },
  });
}

/** What the fixture answers has the shape of the Rust golden, so a screen tested against the one
 * reads the other. */
describe("the access fixture answers in the shape of the Rust goldens", () => {
  const cases: Array<[string, string[], boolean]> = [
    ["access-set-ls.ls", ["access-set", "ls"], false],
    [
      "access-set-add.plan",
      ["access-set", "add", "Deep Work", "--server", "alpha", "--dry-run"],
      false,
    ],
    [
      "access-set-add.apply",
      ["access-set", "add", "Deep Work", "--server", "alpha"],
      false,
    ],
    ["access-set-default.apply", ["access-set", "default", "wide"], false],
    ["access-set-tools.ls", ["access-set", "tools"], false],
    [
      "access-set-tools.apply",
      ["access-set", "tools", "wide", "alpha", "--tool", "read", "--tool", "list"],
      false,
    ],
    ["client-scope.apply", ["client", "scope", "claude-code", "wide"], false],
    ["folder-route-ls.ls", ["folder", "route", "ls"], false],
    ["folder-route-add.apply", ["folder", "route", "add", "/work/other", "bare"], false],
    ["folder-route-rm.apply", ["folder", "route", "rm", "/work/proj"], false],
    ["access-set-rm.in-use", ["access-set", "rm", "bare", "--dry-run"], true],
    ["access-set-rm.detach", ["access-set", "rm", "bare", "--detach"], false],
  ];
  for (const [name, argv, fails] of cases) {
    it(`test_${name.replace(/[.-]/g, "_")}`, () => {
      const expected = golden(name).envelope;
      const reply = world().run(argv);
      expect(reply.ok).toBe(!fails);
      expect(keys(reply.data)).toEqual(keys(expected.data));
      if (fails) expect(reply.error?.code).toBe(expected.error?.code);
      const plan = (expected.data as { plan?: object }).plan;
      if (plan) {
        expect(keys((reply.data as { plan: object }).plan)).toEqual(keys(plan));
      }
    });
  }

  it("test_a_deleted_set_comes_back_with_the_answers_of_the_golden_restore", () => {
    const fixture = world();
    const removed = fixture.run(["access-set", "rm", "bare", "--detach"]);
    const token = (removed.data as { result: { undo: string } }).result.undo
      .split(" ")
      .pop()!;
    expect(token).toMatch(/^removed-[0-9a-f]{16}$/);
    for (const [name, argv] of [
      [
        "access-set-rm.restore-plan",
        ["access-set", "add", "--restore", token, "--dry-run"],
      ],
      ["access-set-rm.restore", ["access-set", "add", "Bare", "--restore", token]],
    ] as const) {
      const expected = golden(name).envelope;
      const reply = fixture.run([...argv]);
      expect(reply.ok, name).toBe(true);
      expect(keys(reply.data), name).toEqual(keys(expected.data));
    }
    const gone = fixture.run(["access-set", "add", "--restore", token]);
    expect(gone.error?.code).toBe(
      golden("access-set-rm.restore-gone").envelope.error?.code,
    );
  });

  it("test_every_golden_of_the_writers_is_known_to_this_file", () => {
    const names = readdirSync(join(root, "ctl-envelopes")).filter((file) =>
      /^(access-set|client-scope|folder-route)-/.test(file),
    );
    expect(names.length).toBeGreaterThan(cases.length);
  });
});
