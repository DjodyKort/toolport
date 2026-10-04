import { describe, expect, it } from "vitest";
import type {
  ClientSyncData,
  ProfileCreateData,
  ProfileEditData,
  ProfileRmData,
  ServerUninstallData,
} from "../bridge/data";
import type {
  ClientDirectAddData,
  ClientDirectRmData,
  ClientEditData,
  ClientImportData,
} from "../types/client";
import { isPlanV1, type PlanV1 } from "../ui/plan";
import { infoOf, serversCtlFixtures, serversWorld } from "../fixtures/servers";
import { clone } from "./testkit";
import {
  clientEditPlan,
  clientImportPlan,
  clientSyncPlan,
  directAddPlan,
  directRmPlan,
  editServerPlan,
  installPlan,
  newServerPlan,
  newServerResult,
  profileCreatePlan,
  profileEditPlan,
  profileRmPlan,
  reAddCommand,
  uninstallPlan,
} from "./plans";

const fixture = <T>(key: string) => clone(serversCtlFixtures.get(key)) as T;
const details = (plan: PlanV1) => plan.steps.map((step) => step.detail);
const work = serversWorld.profileLs.profiles[1];

describe("uninstallPlan", () => {
  const preview = fixture<ServerUninstallData>("server uninstall srv-erp --dry-run");
  const applied = fixture<ServerUninstallData>("server uninstall srv-erp");
  const info = infoOf("srv-erp");

  it("words the dry run in the present tense, with the secrets it would remove", () => {
    const plan = uninstallPlan(preview, false, { info });
    expect(isPlanV1(plan)).toBe(true);
    expect(plan.summary).toBe("Remove the server acme-erp");
    expect(details(plan)).toEqual([
      "Server acme-erp (srv-erp) is removed from the registry",
      "Remove acme-erp from claude-code",
      "Remove secrets from the vault: ERP_API_KEY",
    ]);
    expect(plan.steps[1].path).toBe("/fixture/home/.claude/.claude.json");
  });

  it("gives the command that adds the server back, with its launch line", () => {
    expect(uninstallPlan(preview, false, { info }).undo).toBe(
      "toolportctl server new acme-erp --command acme-erp-mcp --arg --stdio",
    );
    expect(uninstallPlan(preview, false).undo).toBe("");
  });

  it("words the result in the past tense and names the backup", () => {
    const plan = uninstallPlan(applied, true, { info });
    expect(plan.summary).toBe("Removed the server acme-erp");
    expect(details(plan)).toContain(
      "Server acme-erp (srv-erp) was removed from the registry",
    );
    expect(details(plan)).toContain("Removed secrets from the vault: ERP_API_KEY");
    expect(plan.steps.find((step) => step.op === "create")?.path).toBe(
      "/fixture/home/.claude/.claude.json.toolport-bak",
    );
  });

  it("says what the kept options leave alone, and reports a client it could not edit", () => {
    const failing = clone(preview);
    failing.clients[0] = {
      ...(failing.clients[0] as object),
      removed: [],
      error: "config is read-only",
    };
    const plan = uninstallPlan(failing, false, {
      info,
      keepClients: true,
      keepSecrets: true,
    });
    expect(details(plan)).toEqual([
      "Server acme-erp (srv-erp) is removed from the registry",
      "Client entries are kept (--keep-clients)",
      "Its secrets stay in the vault (--keep-secrets)",
    ]);
    expect(plan.warnings).toEqual(["claude-code: config is read-only"]);
  });

  it("shows the address of a server that is reached by url when it is added back", () => {
    const http = infoOf("srv-corp");
    expect(reAddCommand(http)).toBe(
      "toolportctl server new corp-tools --url https://example.test/corp-tools/mcp --transport http",
    );
    expect(reAddCommand(null)).toBe("");
  });
});

describe("profile plans", () => {
  const edit = (
    change: Partial<ProfileEditData["servers"]>,
    rest: Partial<ProfileEditData> = {},
  ) => {
    const base = fixture<ProfileEditData>(
      "profile edit work --add-server srv-docs --dry-run",
    );
    return {
      ...base,
      ...rest,
      servers: { ...base.servers, ...change },
    } as ProfileEditData;
  };

  it("names the added server, the clients that use the profile, and the undo", () => {
    const plan = profileEditPlan(
      fixture<ProfileEditData>("profile edit work --add-server srv-docs --dry-run"),
      false,
      { users: ["cursor"] },
    );
    expect(details(plan)).toEqual([
      "Add srv-docs",
      "Used by cursor: their tool list changes with the next session",
    ]);
    expect(plan.undo).toBe("toolportctl profile edit work --remove-server srv-docs");
  });

  it("undoes an add and a remove together by setting the whole list back", () => {
    const plan = profileEditPlan(
      edit({ added: ["srv-docs"], removed: ["srv-wiki"] }),
      false,
    );
    expect(plan.undo).toBe(
      "toolportctl profile edit work --set-servers srv-corp,srv-issues,srv-wiki",
    );
    const removal = profileEditPlan(edit({ added: [], removed: ["srv-wiki"] }), true);
    expect(removal.summary).toBe("Edited the profile Work");
    expect(details(removal)).toEqual(["Removed srv-wiki"]);
    expect(removal.undo).toBe("toolportctl profile edit work --add-server srv-wiki");
  });

  it("keeps the old name in the undo of a rename and says when nothing changes", () => {
    const renamed = profileEditPlan(
      edit(
        { added: [], removed: [] },
        { renamed: true, oldName: "Work", name: "Office" },
      ),
      false,
    );
    expect(details(renamed)[0]).toBe("Rename the profile from Work to Office");
    expect(renamed.undo).toBe("toolportctl profile edit work --name Work");
    const same = profileEditPlan(
      edit({ added: [], removed: [] }, { changed: false }),
      false,
    );
    expect(same.summary).toBe("The profile Work already matches");
    expect(same.undo).toBe("");
    const left = profileEditPlan(edit({}, { notInProfile: ["srv-x"] }), false);
    expect(left.warnings).toEqual(["Not in the profile, so left alone: srv-x"]);
  });

  it("says what creating a profile does, and that an existing one is kept", () => {
    const made = profileCreatePlan(
      fixture<ProfileCreateData>("profile create demo --dry-run"),
      false,
    );
    expect(made.summary).toBe("Create the profile demo");
    expect(made.undo).toBe("toolportctl profile rm demo");
    expect(
      profileCreatePlan(fixture<ProfileCreateData>("profile create demo"), true).summary,
    ).toBe("Created the profile demo");
    const exists = profileCreatePlan(
      { created: false, dryRun: true, id: "demo", name: "demo" },
      false,
    );
    expect(exists.summary).toBe("Profile demo already exists");
  });

  it("deletes a profile with the client entries scoped to it, and can bring it back", () => {
    const data: ProfileRmData = {
      clients: [
        { client: "cursor", name: "Cursor", backup: "/fixture/cursor.bak", error: null },
        {
          client: "gemini-cli",
          name: "Gemini CLI",
          backup: null,
          error: "config is read-only",
        },
      ],
      dryRun: true,
      id: "work",
      left: ["claude-desktop"],
      name: "Work",
      servers: 3,
    } as unknown as ProfileRmData;
    const plan = profileRmPlan(data, false, { profile: work });
    expect(details(plan)).toEqual([
      "Delete the profile Work (3 servers stay in the registry)",
      "Remove the toolport entry scoped to Work from Cursor",
      "Backup of the client config",
    ]);
    expect(plan.warnings).toEqual([
      "Gemini CLI: config is read-only",
      "claude-desktop still points at this profile and keeps its entry",
    ]);
    expect(plan.undo).toBe(
      "toolportctl profile create Work && toolportctl profile edit Work --add-server corp-tools,issue-tracker,wiki-reader",
    );
    expect(profileRmPlan(data, true).summary).toBe("Deleted the profile Work");
  });
});

describe("client plans", () => {
  it("points a client at a profile, and undoes it with the profiles it had", () => {
    const plan = clientEditPlan(
      fixture<ClientEditData>("client edit gemini-cli --set-profiles work --dry-run"),
      false,
    );
    expect(plan.summary).toBe("Set the profile of Gemini CLI");
    expect(details(plan)[0]).toBe("Point Gemini CLI at Work");
    expect(plan.undo).toBe("toolportctl client edit gemini-cli --remove-profile Work");
    const before = clone(
      fixture<ClientEditData>("client edit gemini-cli --set-profiles work --dry-run"),
    );
    before.profiles = { ...before.profiles, before: ["Research"] };
    expect(clientEditPlan(before, false).undo).toBe(
      "toolportctl client edit gemini-cli --set-profiles Research",
    );
    expect(clientEditPlan({ ...before, changed: false }, false).summary).toBe(
      "Gemini CLI already uses these profiles",
    );
  });

  it("lists what a sync adds, takes out and leaves alone, per client", () => {
    const plan = clientSyncPlan(fixture<ClientSyncData>("client sync --dry-run"), false);
    expect(plan.summary).toBe("Sync the managed clients");
    expect(details(plan)).toEqual([
      "claude-code: leaves the direct launcher entries docs-search alone",
      "cursor: remove the direct entry docs-search (redundant)",
      "cursor: remove the direct entry legacy-lint (orphan)",
    ]);
    const done = clientSyncPlan(fixture<ClientSyncData>("client sync"), true);
    expect(details(done)).toContain(
      "cursor: removed the direct entry legacy-lint (orphan)",
    );
    expect(done.steps.at(-1)).toMatchObject({
      op: "create",
      path: "/fixture/home/.cursor/mcp.json.toolport-bak",
    });
    expect(done.undo).toMatch(/backup/);
  });

  it("says when every client is already in sync, and when the Toolport entry is added", () => {
    const quiet = clientSyncPlan({ dryRun: true, clients: [] }, false);
    expect(details(quiet)).toEqual(["Every client is in sync"]);
    const install = clientSyncPlan(
      {
        dryRun: true,
        clients: [
          {
            client: "gemini-cli",
            gateway: "would-install",
            removed: [],
            kept: ["x"],
            direct: [],
          },
        ],
      },
      false,
    );
    expect(details(install)).toEqual([
      "gemini-cli: Add the Toolport entry",
      "gemini-cli: keeps the orphan entries x",
    ]);
  });

  it("previews an import with what it registers and what it skips", () => {
    const data = fixture<ClientImportData>(
      "client import cursor --select legacy-lint --dry-run",
    );
    const plan = clientImportPlan(
      {
        ...data,
        skipped: [["token-tool", "inline credential"]],
        profile: { name: "Imported", created: true, servers: ["legacy-lint"] },
        secrets: [{ server: "legacy-lint", key: "LINT_TOKEN" }],
      } as unknown as ClientImportData,
      false,
    );
    expect(plan.summary).toBe("Import 1 server from Cursor");
    expect(details(plan)).toEqual([
      "Import legacy-lint into the registry",
      "Create the profile Imported with 1 server",
      "Registers the environment key LINT_TOKEN of legacy-lint",
    ]);
    expect(plan.warnings).toEqual(["token-tool is skipped: inline credential"]);
    expect(clientImportPlan({ ...data, imported: [] }, false).steps[0].detail).toBe(
      "Nothing to import",
    );
  });

  it("states the trade-off of a direct entry and how to remove it", () => {
    const plan = directAddPlan(
      fixture<ClientDirectAddData>(
        "client direct add srv-docs --client cursor --dry-run",
      ),
      false,
    );
    expect(plan.summary).toBe("Add a direct entry for docs-search in Cursor");
    expect(details(plan)[0]).toMatch(
      /^Give Cursor its own entry docs-search for docs-search, started by /,
    );
    expect(plan.warnings[0]).toMatch(/bypasses the Toolport gateway/);
    expect(plan.undo).toBe("toolportctl client direct rm srv-docs --client cursor");
  });

  it("removes a direct entry and can add it again", () => {
    const plan = directRmPlan(
      fixture<ClientDirectRmData>("client direct rm srv-docs --client claude-code"),
      true,
    );
    expect(plan.summary).toBe("Removed a direct entry of docs-search in Claude Code");
    expect(plan.undo).toBe("toolportctl client direct add srv-docs --client claude-code");
  });
});

describe("plans of the commands that have no dry run", () => {
  const fields = {
    name: "notes-index",
    kind: "command" as const,
    command: "node",
    args: ["index.js", "--quiet"],
    url: "",
    transport: "",
    cwd: "/work/notes",
  };

  it("shows what server new would add, from the form", () => {
    const plan = newServerPlan(fields);
    expect(plan.summary).toBe("Add the server notes-index");
    expect(details(plan)).toEqual([
      "Add the server notes-index: node index.js --quiet",
      "Working directory: /work/notes",
      "It is not in any profile yet; secrets and logins are set after it exists",
    ]);
    expect(plan.undo).toBe("toolportctl server uninstall notes-index");
    expect(newServerResult({ id: "srv-new", name: "notes-index" }, "srv-new").undo).toBe(
      "toolportctl server uninstall srv-new",
    );
  });

  it("shows the address of a url server and warns before a catalog program runs", () => {
    const remote = newServerPlan({
      ...fields,
      kind: "url",
      url: "https://example.test/mcp",
      cwd: "",
    });
    expect(details(remote)[0]).toBe(
      "Add the server notes-index: https://example.test/mcp",
    );
    const plan = installPlan({
      name: "docs-search",
      source: "catalog",
      transport: "stdio",
      command: "node",
      args: ["server.js"],
      url: null,
      envKeys: ["DOCS_ROOT"],
    });
    expect(details(plan)).toEqual([
      "Add docs-search from the catalog: node server.js",
      "It needs DOCS_ROOT; set them after installing",
    ]);
    expect(plan.warnings).toEqual([
      "It starts a program on this computer: node server.js",
    ]);
    expect(plan.undo).toBe("toolportctl server uninstall docs-search");
  });

  it("shows each changed field as a before and after, and the way back when there is one", () => {
    const changes = [{ field: "command", before: "node", after: "bun" }];
    const plan = editServerPlan("docs-search", "srv-docs", changes, [
      "server",
      "edit",
      "srv-docs",
      "--command",
      "node",
    ]);
    expect(plan.steps).toEqual([
      {
        op: "update",
        detail: "Change command",
        keys: ["command"],
        diff: { before: "node", after: "bun" },
      },
    ]);
    expect(plan.undo).toBe("toolportctl server edit srv-docs --command node");
    expect(editServerPlan("docs-search", "srv-docs", changes, null).undo).toBe("");
  });
});
