import { describe, expect, it } from "vitest";
import {
  basisText,
  countsText,
  needsLook,
  plain,
  tokenText,
  totalTokens,
  visibleTotals,
} from "./model";
import { summaryData } from "./testkit";

describe("token text", () => {
  it("labels an estimate, a measured number and a projected one", () => {
    expect(tokenText({ value: 1234, basis: "estimate" })).toBe(
      "about 1,234 tokens (estimate)",
    );
    expect(tokenText({ value: 8651, basis: "measured" })).toBe("8,651 tokens (measured)");
    expect(tokenText({ value: 40639, basis: "projected" })).toBe(
      "40,639 tokens (projected by Claude Code)",
    );
    expect(basisText("estimate")).toBe("estimate");
  });

  it("calls a sum measured only when every part is", () => {
    const rows = summaryData().sources;
    expect(totalTokens(rows).basis).toBe("estimate");
    const measured = rows.map((row) => ({
      ...row,
      tokens: { ...row.tokens, basis: "measured" as const },
    }));
    expect(totalTokens(measured).basis).toBe("measured");
    expect(
      totalTokens([measured[0], { ...measured[1], tokens: rows[1].tokens }]).basis,
    ).toBe("estimate");
    expect(totalTokens([])).toEqual({ value: 0, basis: "estimate" });
  });
});

describe("source rows", () => {
  it("sums what Claude can see and lists what wants a look", () => {
    const rows = summaryData().sources;
    expect(visibleTotals(rows)).toEqual({ seen: 18, total: 19 });
    expect(needsLook(rows)).toEqual([
      "1 behind its remote",
      "1 out of date",
      "1 with a duplicate clone",
      "1 skills hidden from Claude",
    ]);
  });

  it("says what a source holds in plain words", () => {
    expect(countsText({ skill: 1, command: 2, agent: 0, rule: 0, memory: 1 })).toBe(
      "1 skill · 2 commands · 1 CLAUDE.md file",
    );
    expect(countsText({ skill: 0, command: 0, agent: 0, rule: 0, memory: 0 })).toBe(
      "nothing found",
    );
  });

  it("drops the user and password of a URL and keeps everything else", () => {
    expect(plain("clone of https://deploy:s3cret@git.example.test/a/b.git")).toBe(
      "clone of https://git.example.test/a/b.git",
    );
    expect(plain("ssh://git@host/repo and git@host:repo")).toBe(
      "ssh://host/repo and git@host:repo",
    );
    expect(plain("origin/main:.claude/skills/odh/SKILL.md")).toBe(
      "origin/main:.claude/skills/odh/SKILL.md",
    );
  });
});
