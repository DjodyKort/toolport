import { describe, expect, it } from "vitest";
import {
  applyArgv,
  applyCommand,
  bundleFlags,
  bundleParts,
  emptyBundleForm,
  formOf,
  listOf,
  nameProblem,
} from "./bundleModel";
import { bundleShow } from "./tabsKit";
import type { ContextBundleShowData } from "../types/context-bundle";

const acme = () => formOf(bundleShow("acme-dev") as ContextBundleShowData);

describe("listOf", () => {
  it("splits on lines and commas, trims and drops empty entries", () => {
    expect(listOf(" a, b\n\nc ,, \n")).toEqual(["a", "b", "c"]);
    expect(listOf("")).toEqual([]);
  });

  it("keeps a glob whole", () => {
    expect(listOf("**/acme-erp/CLAUDE.md")).toEqual(["**/acme-erp/CLAUDE.md"]);
  });
});

describe("formOf", () => {
  it("turns a definition into the lines of the form", () => {
    const form = acme();
    expect(form.name).toBe("acme-dev");
    expect(form.servers).toBe("acme-dev");
    expect(form.skillsOff).toBe("notes-helper\nscratch-*");
    expect(form.skillsNameOnly).toBe("long-guide");
    expect(form.pluginsOff).toBe("tools-pack@tools-market\nloop-runner@official");
    expect(form.layersExclude).toBe("**/acme-erp/CLAUDE.md");
    expect(form.bind).toBe("~/work/acme-erp/clients/*");
  });

  it("reads an unpaired server set as empty", () => {
    expect(formOf(bundleShow("default") as ContextBundleShowData).servers).toBe("");
  });
});

describe("bundleFlags", () => {
  it("for an add sends every filled field and skips the empty ones", () => {
    expect(
      bundleFlags({
        ...emptyBundleForm,
        name: "ops",
        description: " Ops ",
        skillsOff: "a\nb",
        bind: "~/ops/*",
      }),
    ).toEqual(["--description", "Ops", "--skills-off", "a,b", "--bind", "~/ops/*"]);
    expect(bundleFlags({ ...emptyBundleForm, name: "bare" })).toEqual([]);
  });

  it("for an edit sends only the fields that differ from the definition", () => {
    const before = acme();
    expect(bundleFlags(before, before)).toEqual([]);
    expect(
      bundleFlags({ ...before, agentsOff: "reviewer-bot\nplanner" }, before),
    ).toEqual(["--agents-off", "reviewer-bot,planner"]);
    expect(bundleFlags({ ...before, description: "New" }, before)).toEqual([
      "--description",
      "New",
    ]);
  });

  it("clears a list or the server set by sending the flag with an empty value", () => {
    const before = acme();
    expect(bundleFlags({ ...before, pluginsOff: "" }, before)).toEqual([
      "--plugins-off",
      "",
    ]);
    expect(bundleFlags({ ...before, servers: " " }, before)).toEqual(["--servers", ""]);
  });

  it("never reorders or merges lists: each given list replaces the stored one", () => {
    const before = acme();
    expect(bundleFlags({ ...before, skillsOff: "z" }, before)).toEqual([
      "--skills-off",
      "z",
    ]);
  });
});

describe("nameProblem", () => {
  it("accepts a fresh name and says nothing for an empty one", () => {
    expect(nameProblem("", ["a"])).toBeNull();
    expect(nameProblem("ops-lite", ["acme-dev"])).toBeNull();
    expect(nameProblem("a.b_c-1", [])).toBeNull();
  });

  it("refuses a name that starts with a dash, holds a space or is taken", () => {
    expect(nameProblem("-x", [])).toMatch(/Letters, digits/);
    expect(nameProblem("a b", [])).toMatch(/Letters, digits/);
    expect(nameProblem("acme-dev", ["acme-dev"])).toBe(
      "A profile with this name exists.",
    );
  });
});

describe("bundleParts", () => {
  const none = {
    skills: { off: 0, nameOnly: 0, allow: 0 },
    plugins: { off: [] },
    layers: { add: [], exclude: [] },
    agents: { off: [] },
  };

  it("says nothing for a profile that changes nothing", () => {
    expect(bundleParts(none)).toEqual([]);
  });

  it("counts the skills, plugins and agents and mentions layers", () => {
    expect(
      bundleParts({
        skills: { off: 2, nameOnly: 1, allow: 0 },
        plugins: { off: ["a@m", "b@m"] },
        layers: { add: ["x"], exclude: [] },
        agents: { off: ["r"] },
      }),
    ).toEqual(["3 skills changed", "2 plugin(s) off", "layers", "1 agent(s) off"]);
  });

  it("describes a legacy allow list as only N skills", () => {
    expect(bundleParts({ ...none, skills: { off: 0, nameOnly: 0, allow: 1 } })).toEqual([
      "only 1 skill",
    ]);
  });
});

describe("applyArgv", () => {
  it("applies the bundle and its server set with context use when a set is paired", () => {
    expect(applyArgv({ name: "acme-dev", servers: "acme-dev" }, "/f")).toEqual([
      "context",
      "use",
      "acme-dev",
      "--cwd",
      "/f",
    ]);
    expect(applyCommand({ servers: "acme-dev" })).toBe("context use");
  });

  it("uses context bundle apply when there is no server set, and never sends --home", () => {
    const argv = applyArgv({ name: "default", servers: null }, "/f");
    expect(argv).toEqual(["context", "bundle", "apply", "default", "--cwd", "/f"]);
    expect(argv).not.toContain("--home");
    expect(applyCommand({ servers: null })).toBe("context bundle apply");
  });
});
