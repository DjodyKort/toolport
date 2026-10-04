import { describe, expect, it } from "vitest";
import { infoOf, serversWorld } from "../fixtures/servers";
import {
  argList,
  editPlan,
  emptyForm,
  fieldsOf,
  formOf,
  newProblems,
  newServerArgv,
  profileEditArgv,
} from "./forms";

const stdio = infoOf("srv-docs");
const http = infoOf("srv-corp");
const work = serversWorld.profileLs.profiles[1];

describe("the add-server form", () => {
  const form = (change: Partial<ReturnType<typeof emptyForm>>) => ({
    ...emptyForm(),
    ...change,
  });

  it("asks for what is missing before anything runs", () => {
    expect(newProblems(emptyForm(), [])).toEqual([
      "Give the server a name",
      "Give the command that starts it",
    ]);
    expect(
      newProblems(form({ name: "Docs-Search", command: "node" }), ["docs-search"]),
    ).toEqual(["A server called Docs-Search already exists"]);
    expect(
      newProblems(form({ name: "x", kind: "url", url: "example.test" }), []),
    ).toEqual(["Give the address, starting with http:// or https://"]);
    expect(
      newProblems(form({ name: "x", kind: "url", url: "https://example.test/mcp" }), []),
    ).toEqual([]);
  });

  it("sends one token per value, because the CLI takes no `--flag=value` here", () => {
    const command = fieldsOf(
      form({
        name: " notes ",
        command: "node",
        args: "index.js\n\n --quiet \n",
        cwd: "/work",
      }),
    );
    expect(command.args).toEqual(["index.js", "--quiet"]);
    expect(newServerArgv(command)).toEqual([
      "server",
      "new",
      "notes",
      "--command",
      "node",
      "--arg",
      "index.js",
      "--arg",
      "--quiet",
      "--cwd",
      "/work",
    ]);
    const remote = fieldsOf(
      form({
        name: "docs",
        kind: "url",
        url: "https://example.test/mcp",
        transport: "sse",
      }),
    );
    expect(newServerArgv(remote)).toEqual([
      "server",
      "new",
      "docs",
      "--url",
      "https://example.test/mcp",
      "--transport",
      "sse",
    ]);
    expect(argList("a\n\nb")).toEqual(["a", "b"]);
  });
});

describe("editPlan", () => {
  it("sends nothing when nothing changed", () => {
    const plan = editPlan(stdio, formOf(stdio));
    expect(plan).toMatchObject({ changes: [], problems: [], undo: null });
    expect(plan.argv).toEqual(["server", "edit", "srv-docs"]);
  });

  it("sends the changed fields only and keeps the way back", () => {
    const form = {
      ...formOf(stdio),
      name: "docs-index",
      command: "bun",
      cwd: "/work/docs",
    };
    const plan = editPlan(stdio, form);
    expect(plan.argv).toEqual([
      "server",
      "edit",
      "srv-docs",
      "--name",
      "docs-index",
      "--command",
      "bun",
      "--cwd",
      "/work/docs",
    ]);
    expect(plan.changes.map((change) => change.field)).toEqual([
      "name",
      "command",
      "working folder",
    ]);
    expect(plan.undo).toBeNull();
  });

  it("is reversible when every changed field had a value before", () => {
    const plan = editPlan(stdio, { ...formOf(stdio), command: "bun", args: "other.js" });
    expect(plan.argv).toEqual([
      "server",
      "edit",
      "srv-docs",
      "--command",
      "bun",
      "--arg",
      "other.js",
    ]);
    expect(plan.undo).toEqual([
      "server",
      "edit",
      "srv-docs",
      "--command",
      "node",
      "--arg",
      "server.js",
      "--arg",
      "--quiet",
    ]);
  });

  it("replaces the whole argument list, and says what it cannot do instead of sending it", () => {
    const emptied = editPlan(stdio, { ...formOf(stdio), args: "" });
    expect(emptied.problems).toEqual(["The arguments cannot be emptied with the CLI"]);
    expect(emptied.argv).toEqual(["server", "edit", "srv-docs"]);
    const noCwd = editPlan({ ...stdio, cwd: "/work" }, { ...formOf(stdio), cwd: "" });
    expect(noCwd.problems).toEqual(["working folder cannot be emptied with the CLI"]);
    const noCommand = editPlan(stdio, { ...formOf(stdio), command: "" });
    expect(noCommand.problems).toEqual(["Give the command that starts it"]);
    expect(editPlan(stdio, { ...formOf(stdio), name: "" }).problems).toContain(
      "Give the server a name",
    );
  });

  it("edits the address of a url server and checks it", () => {
    const plan = editPlan(http, { ...formOf(http), url: "https://example.test/v2/mcp" });
    expect(plan.argv).toEqual([
      "server",
      "edit",
      "srv-corp",
      "--url",
      "https://example.test/v2/mcp",
    ]);
    expect(plan.undo).toEqual([
      "server",
      "edit",
      "srv-corp",
      "--url",
      "https://example.test/corp-tools/mcp",
    ]);
    expect(editPlan(http, { ...formOf(http), url: "not a url" }).problems).toEqual([
      "Give the address, starting with http:// or https://",
    ]);
  });

  it("turns the two options on and off with on|off", () => {
    const plan = editPlan(stdio, {
      ...formOf(stdio),
      declareClientCapabilities: true,
      forwardInstructions: true,
    });
    expect(plan.argv).toEqual([
      "server",
      "edit",
      "srv-docs",
      "--declare-client-capabilities",
      "on",
      "--forward-instructions",
      "on",
    ]);
    expect(plan.undo).toEqual([
      "server",
      "edit",
      "srv-docs",
      "--declare-client-capabilities",
      "off",
      "--forward-instructions",
      "off",
    ]);
  });
});

describe("profileEditArgv", () => {
  const ids = work.servers.map((server) => server.id);

  it("names what is added, or what is removed", () => {
    expect(
      profileEditArgv(work, { name: "Work", servers: [...ids, "srv-docs"] }),
    ).toEqual(["profile", "edit", "work", "--add-server", "srv-docs"]);
    expect(profileEditArgv(work, { name: "Work", servers: ids.slice(1) })).toEqual([
      "profile",
      "edit",
      "work",
      "--remove-server",
      ids[0],
    ]);
  });

  it("sets the whole list when it adds and removes at once, because the CLI takes one option", () => {
    expect(
      profileEditArgv(work, { name: "Work", servers: [ids[1], ids[2], "srv-docs"] }),
    ).toEqual([
      "profile",
      "edit",
      "work",
      "--set-servers",
      `${ids[1]},${ids[2]},srv-docs`,
    ]);
  });

  it("renames next to a server change, and sends only the name when only it changed", () => {
    expect(profileEditArgv(work, { name: " Office ", servers: ids })).toEqual([
      "profile",
      "edit",
      "work",
      "--name",
      "Office",
    ]);
    expect(
      profileEditArgv(work, { name: "Office", servers: [...ids, "srv-docs"] }),
    ).toEqual([
      "profile",
      "edit",
      "work",
      "--name",
      "Office",
      "--add-server",
      "srv-docs",
    ]);
  });
});
