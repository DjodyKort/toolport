import { describe, expect, it } from "vitest";
import type { CommandRow } from "../bridge/data";
import { clone } from "./testkit";
import { commandsServed, serversWorld } from "../fixtures/servers";
import {
  buildClientViews,
  buildServerViews,
  gatewayServers,
  groupOf,
  groupServers,
  initials,
  launchLine,
  looksLikeLogin,
  namedFix,
  policyOf,
  profileUsers,
  serverSummary,
  stateLabel,
  stateTone,
  stripFacts,
  type ServerInfo,
  type StatusDoc,
} from "./model";

const { serverLs, profileLs, status, clientLs } = serversWorld;
const rows = commandsServed.commands;

function viewsOf(change?: (world: { ls: typeof serverLs; doc: StatusDoc }) => void) {
  const world = { ls: clone(serverLs), doc: clone(status) };
  change?.(world);
  return buildServerViews(world.ls, profileLs, world.doc);
}

const byName = (views: ReturnType<typeof viewsOf>, name: string) =>
  views.find((view) => view.name === name)!;

describe("buildServerViews", () => {
  it("states every server of the fixture registry as the CLI and the gateway report it", () => {
    const views = viewsOf();
    expect(
      Object.fromEntries(views.map((view) => [view.name, [view.state, view.tools]])),
    ).toEqual({
      "docs-search": ["connected", 14],
      "corp-tools": ["connected", 28],
      "wiki-reader": ["connected", 31],
      "mail-bridge": ["connected", 9],
      "issue-tracker": ["login", null],
      "design-files": ["login", null],
      "acme-erp": ["failed", null],
      "scratch-notes": ["disabled", null],
    });
    expect(byName(views, "issue-tracker").reason).toBe(
      "Sign-in needed: HTTP 401 invalid_token",
    );
    expect(byName(views, "acme-erp").reason).toBe("The gateway could not start it");
    expect(byName(views, "docs-search").profiles.map((profile) => profile.id)).toEqual([
      "default",
      "research",
    ]);
  });

  it("matches a login row to its server by id or by name, as `status` words it", () => {
    const byId = viewsOf(({ doc }) => {
      doc.auth.servers[4] = { ...doc.auth.servers[4], server: "srv-issues" };
    });
    expect(byName(byId, "issue-tracker").state).toBe("login");
    const byCase = viewsOf(({ doc }) => {
      doc.auth.servers[4] = { ...doc.auth.servers[4], server: "Issue-Tracker" };
    });
    expect(byName(byCase, "issue-tracker").state).toBe("login");
  });

  it("puts a server that is not in the active profile first, whatever else is wrong with it", () => {
    const views = viewsOf(({ ls }) => {
      ls.servers.find((server) => server.id === "srv-issues")!.enabled = false;
    });
    expect(byName(views, "issue-tracker").state).toBe("disabled");
    expect(byName(views, "issue-tracker").reason).toBeNull();
  });

  it("lets a login win over a failed start, and a failed start over a stale connection", () => {
    const failedAndLogin = viewsOf();
    expect(byName(failedAndLogin, "issue-tracker").state).toBe("login");
    const misconfigured = viewsOf(({ doc }) => {
      doc.auth.servers[0] = {
        ...doc.auth.servers[0],
        state: "misconfigured",
        reason: "bad url",
      };
    });
    expect(byName(misconfigured, "docs-search")).toMatchObject({
      state: "failed",
      reason: "The sign-in setup is wrong: bad url",
    });
    const revoked = viewsOf(({ doc }) => {
      doc.auth.servers[0] = {
        ...doc.auth.servers[0],
        state: "revoked",
        reason: "revoked",
      };
    });
    expect(byName(revoked, "docs-search")).toMatchObject({
      state: "login",
      reason: "Access was revoked",
    });
  });

  it("marks a login that still works but is about to end as expiring and keeps it connected", () => {
    const views = viewsOf(({ doc }) => {
      doc.auth.servers[1] = {
        ...doc.auth.servers[1],
        state: "expiring",
        reason: "expiring",
      };
    });
    const view = byName(views, "corp-tools");
    expect(view).toMatchObject({ state: "connected", expiring: true, tools: 28 });
    expect(groupOf(view)).toBe("attention");
    expect(stateLabel(view)).toBe("Login expiring");
    expect(stateTone(view)).toBe("warning");
  });

  it("takes the best answer when two gateways know a server differently", () => {
    const doc = clone(status);
    const second = clone(doc.gateway.builds[0]);
    second.servers = [
      { id: "srv-docs", state: "connecting", tools: 0 },
      { id: "srv-erp", state: "connected", tools: 5 },
    ];
    doc.gateway.builds.push(second);
    const best = gatewayServers(doc);
    expect(best.get("srv-docs")).toMatchObject({ state: "connected", tools: 14 });
    expect(best.get("srv-erp")).toMatchObject({ state: "connected", tools: 5 });
    const views = buildServerViews(serverLs, profileLs, doc);
    expect(byName(views, "acme-erp").state).toBe("connected");
  });

  it("says starting while the gateway connects and not started while no gateway runs", () => {
    const starting = viewsOf(({ doc }) => {
      doc.gateway.builds[0].servers[0] = {
        id: "srv-docs",
        state: "connecting",
        tools: 0,
      };
    });
    expect(byName(starting, "docs-search")).toMatchObject({
      state: "starting",
      tools: null,
    });
    expect(serverSummary(byName(starting, "docs-search"))).toBe("Connecting");
    const stopped = viewsOf(({ doc }) => {
      doc.gateway.builds = [];
      doc.gateway.build = null;
    });
    expect(stopped.filter((view) => view.enabled).map((view) => view.state)).toEqual([
      "idle",
      "idle",
      "idle",
      "idle",
      "login",
      "login",
      "idle",
    ]);
  });
});

describe("groupServers", () => {
  it("lists what needs attention first, then what runs, then what does not", () => {
    const groups = groupServers(viewsOf());
    expect(groups.map((entry) => entry.group)).toEqual([
      "attention",
      "connected",
      "waiting",
    ]);
    expect(groups[0].servers.map((view) => view.name)).toEqual([
      "issue-tracker",
      "design-files",
      "acme-erp",
    ]);
    expect(groups[2].servers.map((view) => view.name)).toEqual(["scratch-notes"]);
  });

  it("filters by name or transport and drops the groups it empties", () => {
    const views = viewsOf();
    expect(
      groupServers(views, "  WIKI ").flatMap((g) => g.servers.map((v) => v.name)),
    ).toEqual(["wiki-reader"]);
    expect(groupServers(views, "http").flatMap((g) => g.group)).toEqual([
      "attention",
      "connected",
    ]);
    expect(groupServers(views, "nothing like this")).toEqual([]);
  });

  it("words every state and gives each a summary line", () => {
    const views = viewsOf();
    expect(views.map((view) => stateLabel(view))).toEqual([
      "Connected",
      "Connected",
      "Connected",
      "Connected",
      "Login needed",
      "Login needed",
      "Failed",
      "Not in profile",
    ]);
    expect(serverSummary(byName(views, "docs-search"))).toBe("14 tools");
    expect(serverSummary(byName(views, "scratch-notes"))).toBe(
      "Not in the active profile",
    );
    expect(stateTone(byName(views, "acme-erp"))).toBe("destructive");
    expect(stateTone(byName(views, "scratch-notes"))).toBe("secondary");
  });
});

describe("stripFacts", () => {
  const facts = (doc: StatusDoc, clients = clientLs.clients) => stripFacts(doc, clients);

  it("shows a running gateway, its catalog, the logins that need you and the clients", () => {
    const [gateway, catalog, logins, clients] = facts(status);
    expect(gateway).toMatchObject({
      label: "Gateway",
      value: "Running",
      tone: "success",
    });
    expect(gateway.detail).toBe("daemon, pid 4242, v0.0.0-fixture");
    expect(catalog).toMatchObject({
      value: "82 tools",
      detail: "from 4 of 7 servers, 3 failed",
      tone: "warning",
    });
    expect(logins).toMatchObject({
      value: "2 need a sign-in",
      detail: "issue-tracker, design-files",
    });
    expect(clients).toMatchObject({
      value: "3 managed",
      detail: "4 detected, 1 direct entries",
    });
  });

  it("tells a missing gateway binary, a gateway that is not running and one that is starting apart", () => {
    const missing = clone(status);
    missing.gateway.present = false;
    expect(facts(missing)[0]).toMatchObject({
      value: "Binary missing",
      tone: "destructive",
    });
    const stopped = clone(status);
    stopped.gateway.builds = [];
    stopped.gateway.build = null;
    expect(facts(stopped)[0]).toMatchObject({ value: "Not running", tone: "muted" });
    expect(facts(stopped)[1]).toMatchObject({ value: "No tool cache" });
    const building = clone(status);
    building.gateway.builds[0].building = true;
    building.gateway.build = building.gateway.builds[0];
    expect(facts(building)[0]).toMatchObject({
      value: "Starting",
      detail: "4 of 7 servers connected",
    });
  });

  it("reads every login as fine when none needs a sign-in, and waits for the clients", () => {
    const signedIn = clone(status);
    signedIn.auth.servers = signedIn.auth.servers.filter((row) => row.state === "ok");
    expect(facts(signedIn)[2]).toMatchObject({ value: "All signed in", tone: "success" });
    expect(stripFacts(status, null)[3]).toMatchObject({ value: "Reading", detail: "" });
  });
});

describe("policyOf", () => {
  it("takes the tier and the preview flag from the registry, not from the screen", () => {
    expect(policyOf(rows, "server uninstall")).toEqual({
      id: "server uninstall",
      tier: "destructive",
      previewFlag: "--dry-run",
      terminal: false,
    });
    expect(policyOf(rows, "profile edit")).toMatchObject({
      tier: "write",
      previewFlag: "--dry-run",
    });
    expect(policyOf(rows, "server new")).toMatchObject({
      tier: "write",
      previewFlag: null,
    });
    expect(policyOf(rows, "doctor")).toMatchObject({ tier: "read", previewFlag: null });
  });

  it("knows the commands that need a terminal", () => {
    expect(policyOf(rows, "direct run")?.terminal).toBe(true);
    expect(policyOf(rows, "server uninstall")?.terminal).toBe(false);
  });

  it("has no policy for what the registry does not have, marks as planned or has not read yet", () => {
    expect(policyOf(rows, "server frobnicate")).toBeNull();
    expect(policyOf(null, "server uninstall")).toBeNull();
    const planned = rows.map((row): CommandRow =>
      row.id === "server uninstall" ? { ...row, planned: true } : row,
    );
    expect(policyOf(planned, "server uninstall")).toBeNull();
    expect(policyOf(rows, "server")).toBeNull();
  });
});

describe("buildClientViews", () => {
  const views = viewsOf();
  const clients = buildClientViews(clientLs.clients, profileLs, views);
  const client = (id: string) => clients.find((entry) => entry.id === id)!;

  it("says which profile each client uses and what it sees through it", () => {
    expect(client("claude-code")).toMatchObject({
      followsActive: true,
      tools: 82,
      connectedServers: 4,
    });
    expect(client("claude-code").profile?.name).toBe("Default");
    expect(client("claude-code").seen).toHaveLength(7);
    expect(client("claude-desktop")).toMatchObject({
      followsActive: false,
      tools: 45,
      connectedServers: 2,
    });
    expect(client("claude-desktop").profile?.name).toBe("Research");
    expect(client("cursor")).toMatchObject({ tools: 59, connectedServers: 2 });
    expect(client("cursor").seen.map((row) => row.name)).toEqual([
      "corp-tools",
      "issue-tracker",
      "wiki-reader",
    ]);
  });

  it("splits the entries Toolport did not write into orphans and duplicates of a server", () => {
    expect(client("cursor").orphans).toEqual(["legacy-lint"]);
    expect(client("cursor").redundant).toEqual(["docs-search"]);
    expect(client("claude-code").orphans).toEqual([]);
    expect(client("claude-code").launchers.map((entry) => entry.entry)).toEqual([
      "docs-search",
    ]);
  });

  it("does not guess a profile for a client scoped to one that does not exist", () => {
    const stray = clone(clientLs.clients);
    stray[0].scope = "gone";
    const [first] = buildClientViews(stray, profileLs, views);
    expect(first.profile).toBeNull();
    expect(first.followsActive).toBe(false);
    expect(first.seen).toEqual([]);
  });
});

describe("profileUsers", () => {
  const profile = (id: string) => profileLs.profiles.find((entry) => entry.id === id)!;

  it("names the clients scoped to a profile, and for the active one the clients that follow it", () => {
    expect(profileUsers(profile("work"), false, clientLs.clients)).toEqual(["cursor"]);
    expect(profileUsers(profile("default"), true, clientLs.clients)).toEqual([
      "claude-code",
    ]);
    expect(profileUsers(profile("default"), true, null)).toEqual([]);
  });
});

describe("text helpers", () => {
  it("finds the fix a doctor line names", () => {
    const line = (detail: string) => ({ name: "skills", status: "warn", detail });
    expect(
      namedFix(line("3 files would be hidden; run toolportctl skills sync")),
    ).toEqual(["skills", "sync"]);
    expect(namedFix(line("run toolportctl auth probe"))).toEqual(["auth", "probe"]);
    expect(namedFix(line("everything is fine"))).toBeNull();
    expect(namedFix(line("run toolportctl skills sync and then restart"))).toBeNull();
  });

  it("tells a server that asks for a login from one that is broken", () => {
    for (const message of [
      "HTTP 401 invalid_token",
      "Unauthorized",
      "needs to sign in",
      "OAuth required",
    ]) {
      expect(looksLikeLogin(message), message).toBe(true);
    }
    for (const message of [
      "could not start acme-erp-mcp",
      "connection refused",
      "HTTP 500",
    ]) {
      expect(looksLikeLogin(message), message).toBe(false);
    }
  });

  it("shows the launch line of a command or an address, and short initials", () => {
    const info = {
      command: "node",
      args: ["server.js", "--quiet"],
      url: null,
    } as ServerInfo;
    expect(launchLine(info)).toBe("node server.js --quiet");
    expect(
      launchLine({ ...info, command: null, args: [], url: "https://example.test/mcp" }),
    ).toBe("https://example.test/mcp");
    expect(launchLine({ ...info, command: null, args: [] })).toBe("no launch command");
    expect(initials("docs-search")).toBe("DO");
    expect(initials("--")).toBe("?");
  });
});
