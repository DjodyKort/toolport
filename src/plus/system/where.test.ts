import { describe, expect, it } from "vitest";
import { golden } from "./testkit";
import { loginCounts, parseFlow, secretsBackendLabel } from "./where";

describe("parseFlow", () => {
  it("turns the flow_diagram golden into a heading and three chains", () => {
    const blocks = parseFlow(golden("mcp-call.flow_diagram").result.markdown);
    expect(blocks.map((block) => block.kind)).toEqual([
      "heading",
      "chain",
      "chain",
      "chain",
    ]);
    expect(blocks[0]).toEqual({ kind: "heading", level: 1, text: "Data flow" });
    expect(blocks[1]).toEqual({
      kind: "chain",
      nodes: [
        { label: "canonical skills repository", detail: null },
        { label: "transpilers", detail: null },
        { label: "per-client outputs", detail: null },
      ],
      arrows: ["to", "to"],
    });
    expect(blocks[2]).toMatchObject({
      nodes: [
        { label: "registry", detail: "servers, profiles" },
        { label: "gateway", detail: null },
        { label: "every client", detail: null },
      ],
    });
    expect(blocks[3]).toMatchObject({
      nodes: [
        { label: "encrypted sync bundle", detail: null },
        { label: "remote", detail: "push and pull" },
      ],
      arrows: ["both"],
    });
  });

  it("reads the direction of each arrow and ignores a bullet", () => {
    expect(parseFlow("- a --> b <- c <-> d")).toEqual([
      {
        kind: "chain",
        nodes: ["a", "b", "c", "d"].map((label) => ({ label, detail: null })),
        arrows: ["to", "from", "both"],
      },
    ]);
  });

  it("does not split a word that holds a dash, and keeps other lines whole", () => {
    expect(parseFlow("per-client outputs\n\n## Sub\n")).toEqual([
      { kind: "text", text: "per-client outputs" },
      { kind: "heading", level: 2, text: "Sub" },
    ]);
  });

  it("keeps a fenced block verbatim, even one that never closes", () => {
    expect(parseFlow("```mermaid\nA --> B\n  B --> C\n```")).toEqual([
      { kind: "code", text: "A --> B\n  B --> C" },
    ]);
    expect(parseFlow("```\nA --> B")).toEqual([{ kind: "code", text: "A --> B" }]);
  });

  it("is empty for an empty text", () => {
    expect(parseFlow("")).toEqual([]);
    expect(parseFlow("\n\n")).toEqual([]);
  });
});

describe("where_am_i words", () => {
  it("names the secret store and passes an unknown one through", () => {
    expect(secretsBackendLabel("os-keychain")).toBe("The operating system keychain");
    expect(secretsBackendLabel("vault")).toBe("vault");
    expect(secretsBackendLabel("")).toBe("Unknown");
  });

  it("lists the login states that have servers, what works first", () => {
    expect(loginCounts({ revoked: 1, ok: 3, unknown: 0 })).toEqual([
      { key: "ok", label: "signed in", count: 3, problem: false },
      { key: "revoked", label: "revoked", count: 1, problem: true },
    ]);
    expect(loginCounts({})).toEqual([]);
  });
});
