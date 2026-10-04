import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { serversWorld } from "../fixtures/servers";
import { renderScreen, tab, wire } from "./harness";
import { clone, createBridge, ctlFailure, deferred, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
});

const WRITES = /^(profile (edit|create|rm))/;
const applied = () =>
  bridge.ran().filter((line) => WRITES.test(line) && !line.includes("--dry-run"));

async function openProfiles() {
  const screenView = renderScreen();
  await screen.findByRole("region", { name: "Needs attention" });
  await screenView.user.click(tab("Profiles"));
  const list = await screen.findByRole("list", { name: "Profiles" });
  return { ...screenView, list };
}

describe("servers.profiles: the Profiles tab", () => {
  it("lists every profile with the clients that use it, the active one named as such", async () => {
    const { list } = await openProfiles();
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0]).toHaveTextContent("Default");
    expect(items[0]).toHaveTextContent("active");
    expect(items[0]).toHaveTextContent(
      "Used by clients without their own profile: claude-code",
    );
    expect(items[0]).toHaveTextContent("7 servers");
    expect(items[1]).toHaveTextContent("Work");
    expect(items[1]).toHaveTextContent("Used by cursor");
    expect(items[1]).toHaveTextContent("3 servers");
    expect(items[2]).toHaveTextContent("Research");
    expect(items[2]).toHaveTextContent("Used by claude-desktop");
    expect(screen.getByText(/skips a server that needs a login/)).toBeInTheDocument();
    expect(applied()).toEqual([]);
  });

  it("shows an empty state that offers to create the first profile", async () => {
    const none = clone(serversWorld.profileLs);
    none.profiles = [];
    bridge.set("profile ls", none);
    const { user } = renderScreen();
    await screen.findByRole("region", { name: "Not running" });
    await user.click(tab("Profiles"));
    expect(await screen.findByText("No profiles yet")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /New profile/ })).toHaveLength(2);
  });

  it("shows the CLI's error with Retry when the profiles cannot be read, and recovers", async () => {
    const good = bridge.get("profile ls");
    bridge.set(
      "profile ls",
      ctlFailure("registry_unreadable", "the registry could not be parsed"),
    );
    const { user } = renderScreen();
    await user.click(tab("Profiles"));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Couldn't load the profiles");
    expect(alert).toHaveTextContent("registry_unreadable");
    expect(alert).toHaveTextContent("the registry could not be parsed");
    bridge.set("profile ls", good);
    await user.click(within(alert).getByRole("button", { name: /Retry/ }));
    expect(await screen.findByRole("list", { name: "Profiles" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("servers.profile-create: a new profile", () => {
  it("previews, confirms and creates it", async () => {
    const { user } = await openProfiles();
    await user.click(screen.getByRole("button", { name: /New profile/ }));
    const ask = await screen.findByRole("dialog", { name: "New profile" });
    await user.type(within(ask).getByRole("textbox", { name: "Name" }), "demo");
    await user.click(within(ask).getByRole("button", { name: "Review" }));

    const review = await screen.findByRole("dialog", { name: "Create demo?" });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(
      within(preview).getByText("Profile demo (demo) with no servers"),
    ).toBeInTheDocument();
    expect(within(preview).getByText(/toolportctl profile rm demo/)).toBeInTheDocument();
    expect(bridge.ran()).toContain("profile create demo --dry-run");
    expect(applied()).toEqual([]);
    await user.click(within(review).getByRole("button", { name: "Apply" }));
    await screen.findByText("Created the profile demo");
    expect(applied()).toEqual(["profile create demo"]);
  });

  it("asks for a name and refuses one that is taken, ignoring case", async () => {
    const { user } = await openProfiles();
    await user.click(screen.getByRole("button", { name: /New profile/ }));
    const ask = await screen.findByRole("dialog", { name: "New profile" });
    await user.click(within(ask).getByRole("button", { name: "Review" }));
    expect(await within(ask).findByRole("alert")).toHaveTextContent(
      "Give the profile a name",
    );
    await user.type(within(ask).getByRole("textbox", { name: "Name" }), "WORK");
    expect(within(ask).getByRole("alert")).toHaveTextContent(
      "A profile called WORK already exists",
    );
    expect(bridge.ran().some((line) => line.startsWith("profile create"))).toBe(false);
  });
});

describe("servers.profile-edit: editing a profile", () => {
  async function openEdit(name: string) {
    const view = await openProfiles();
    await view.user.click(screen.getByRole("button", { name: `Edit ${name}` }));
    const edit = await screen.findByRole("dialog", { name: `Edit ${name}` });
    return { ...view, edit };
  }

  it("shows who uses the profile, previews the change with its client impact, then applies", async () => {
    const { user, edit } = await openEdit("Work");
    expect(edit).toHaveAccessibleDescription(
      /Used by cursor\. A change reaches them with their next session\./,
    );
    const boxes = within(edit).getAllByRole("checkbox");
    const nameOf = (box: HTMLElement) =>
      box.closest("label")?.querySelector("span")?.textContent;
    expect(boxes.filter((box) => (box as HTMLInputElement).checked).map(nameOf)).toEqual([
      "corp-tools",
      "wiki-reader",
      "issue-tracker",
    ]);
    const review = within(edit).getByRole("button", { name: "Review changes" });
    expect(review).toBeDisabled();
    await user.click(within(edit).getByRole("checkbox", { name: /docs-search/ }));
    await user.click(review);

    const confirm = await screen.findByRole("dialog", { name: "Edit Work?" });
    const preview = await within(confirm).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain("profile edit work --add-server srv-docs --dry-run");
    expect(within(preview).getByText("Add srv-docs")).toBeInTheDocument();
    expect(
      within(preview).getByText(
        "Used by cursor: their tool list changes with the next session",
      ),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
    await user.click(within(confirm).getByRole("button", { name: "Apply" }));
    await screen.findByText("Edited the profile Work");
    expect(applied()).toEqual(["profile edit work --add-server srv-docs"]);
  });

  it("sets the whole list when it adds and removes in one go, since the CLI takes one option", async () => {
    bridge.set("profile edit work --set-servers srv-corp,srv-issues,srv-docs --dry-run", {
      changed: true,
      dryRun: true,
      id: "work",
      name: "Work",
      notInProfile: [],
      oldName: "Work",
      renamed: false,
      servers: {
        added: ["srv-docs"],
        after: ["srv-corp", "srv-issues", "srv-docs"],
        before: ["srv-corp", "srv-issues", "srv-wiki"],
        removed: ["srv-wiki"],
      },
    });
    const { user, edit } = await openEdit("Work");
    await user.click(within(edit).getByRole("checkbox", { name: /wiki-reader/ }));
    await user.click(within(edit).getByRole("checkbox", { name: /docs-search/ }));
    await user.click(within(edit).getByRole("button", { name: "Review changes" }));
    const confirm = await screen.findByRole("dialog", { name: "Edit Work?" });
    const preview = await within(confirm).findByRole("region", { name: "Preview" });
    expect(within(preview).getByText("Add srv-docs")).toBeInTheDocument();
    expect(within(preview).getByText("Remove srv-wiki")).toBeInTheDocument();
    expect(
      within(preview).getByText(/--set-servers srv-corp,srv-issues,srv-wiki/),
    ).toBeInTheDocument();
  });

  it("refuses an empty name and a name another profile has", async () => {
    const { user, edit } = await openEdit("Work");
    const name = within(edit).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    expect(within(edit).getByRole("alert")).toHaveTextContent("Give the profile a name");
    await user.type(name, "research");
    expect(within(edit).getByRole("alert")).toHaveTextContent(
      "A profile called research already exists",
    );
    expect(within(edit).getByRole("button", { name: "Review changes" })).toBeDisabled();
  });

  it("says the profile is not used when no client points at it", async () => {
    const lonely = clone(serversWorld.profileLs);
    lonely.profiles[1].clients = [];
    const clients = clone(serversWorld.clientLs);
    clients.clients[2].scope = null;
    bridge.set("profile ls", lonely);
    bridge.set("client ls", clients);
    const { edit } = await openEdit("Work");
    expect(edit).toHaveAccessibleDescription(/No client uses this profile yet\./);
  });
});

describe("servers.profile-delete: deleting a profile", () => {
  async function startDelete(name: string) {
    const view = await openProfiles();
    await view.user.click(screen.getByRole("button", { name: `Delete ${name}` }));
    return {
      ...view,
      ask: await screen.findByRole("dialog", { name: `Delete ${name}` }),
    };
  }

  it("shows the client impact first, asks to type the name, then deletes", async () => {
    const { user, ask } = await startDelete("Work");
    expect(ask).toHaveAccessibleDescription(
      /The 3 servers in it stay in the registry\. Used by cursor\./,
    );
    expect(applied()).toEqual([]);
    await user.click(within(ask).getByRole("button", { name: "Preview deletion" }));

    const review = await screen.findByRole("dialog", { name: "Delete Work?" });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(bridge.ran()).toContain("profile rm work --dry-run");
    expect(
      within(preview).getByText(
        "Delete the profile Work (3 servers stay in the registry)",
      ),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText("Remove the toolport entry scoped to Work from Cursor"),
    ).toBeInTheDocument();
    expect(
      within(preview).getByText(
        /toolportctl profile create Work && toolportctl profile edit Work/,
      ),
    ).toBeInTheDocument();

    const confirm = within(review).getByRole("button", { name: "Delete profile" });
    expect(confirm).toBeDisabled();
    const field = within(review).getByRole("textbox", { name: /type Work to confirm/i });
    await user.type(field, "work");
    expect(confirm).toBeDisabled();
    await user.clear(field);
    await user.type(field, "Work");
    await user.click(confirm);
    await screen.findByText("Deleted the profile Work");
    expect(applied()).toEqual(["profile rm work"]);
  });

  it("leaves the client entries in place with --no-clients", async () => {
    bridge.set("profile rm work --no-clients --dry-run", {
      clients: [],
      dryRun: true,
      id: "work",
      left: ["cursor"],
      name: "Work",
      servers: 3,
    });
    const { user, ask } = await startDelete("Work");
    await user.click(within(ask).getByRole("checkbox", { name: /--no-clients/ }));
    await user.click(within(ask).getByRole("button", { name: "Preview deletion" }));
    const review = await screen.findByRole("dialog", { name: "Delete Work?" });
    const preview = await within(review).findByRole("region", { name: "Preview" });
    expect(
      within(preview).getByText(
        "cursor still points at this profile and keeps its entry",
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("profile rm work --no-clients --dry-run");
  });

  it("applies nothing when the preview fails, and shows why", async () => {
    bridge.set(
      "profile rm work --dry-run",
      ctlFailure("conflict", "the active profile cannot be deleted"),
    );
    const { user, ask } = await startDelete("Work");
    await user.click(within(ask).getByRole("button", { name: "Preview deletion" }));
    expect(
      await screen.findByText(/the active profile cannot be deleted/),
    ).toBeInTheDocument();
    expect(applied()).toEqual([]);
  });
});

describe("servers.profile-inspect: inspecting a whole profile", () => {
  async function inspect(name: string) {
    const view = await openProfiles();
    await view.user.click(screen.getByRole("button", { name: `Inspect ${name}` }));
    return {
      ...view,
      dialog: await screen.findByRole("dialog", { name: `Inspect ${name}` }),
    };
  }

  it("returns partial results when one server answers 401, instead of stopping at it", async () => {
    const { dialog } = await inspect("Work");
    expect(
      await within(dialog).findByText(
        /2 of 3 servers answered with 4 tools, 1 need a login\./,
      ),
    ).toBeInTheDocument();
    const rows = Array.from(
      within(dialog).getByRole("list", { name: "Servers in the profile" }).children,
    );
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("corp-tools");
    expect(rows[0]).toHaveTextContent("Answered");
    expect(rows[0]).toHaveTextContent("2 tools");
    expect(rows[1]).toHaveTextContent("issue-tracker");
    expect(rows[1]).toHaveTextContent("Needs a login");
    expect(rows[1]).toHaveTextContent("HTTP 401 invalid_token");
    expect(rows[2]).toHaveTextContent("wiki-reader");
    expect(rows[2]).toHaveTextContent("Answered");
    expect(
      within(dialog).getByText(
        /The whole-profile command stopped at: issue-tracker: HTTP 401 invalid_token/,
      ),
    ).toBeInTheDocument();
    expect(bridge.ran()).toContain("profile inspect work");
    expect(bridge.ran()).toContain("inspect srv-issues");
  });

  it("separates a server that needs a login from one that cannot start", async () => {
    const { dialog } = await inspect("Default");
    await within(dialog).findByText(/4 of 7 servers answered/);
    expect(within(dialog).getByText(/2 need a login, 1 failed\./)).toBeInTheDocument();
    const rows = Array.from(
      within(dialog).getByRole("list", { name: "Servers in the profile" }).children,
    );
    const erp = rows.find((item) => item.textContent?.includes("acme-erp"))!;
    expect(erp).toHaveTextContent("Failed");
    expect(erp).toHaveTextContent("could not start acme-erp-mcp");
  });

  it("uses the single command when every server answers, and lists the tools on request", async () => {
    const { user, dialog } = await inspect("Research");
    await within(dialog).findByText(/3 of 3 servers answered with 6 tools\./);
    expect(within(dialog).queryByText(/stopped at/)).not.toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "Open Logins" }),
    ).not.toBeInTheDocument();
    expect(bridge.ran().filter((line) => line.startsWith("inspect "))).toEqual([]);
    await user.click(within(dialog).getByText("Show the tools of docs-search"));
    expect(within(dialog).getByText("search_docs")).toBeVisible();
  });

  it("offers to open Logins when a server needs one", async () => {
    const { user, dialog } = await inspect("Work");
    await within(dialog).findByText(/1 need a login/);
    await user.click(within(dialog).getByRole("button", { name: "Open Logins" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Logins" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("shows progress while servers connect and stops when it is closed", async () => {
    const slow = deferred<unknown>();
    bridge.set("profile inspect research", () => slow.promise);
    const { user, dialog } = await inspect("Research");
    expect(within(dialog).getByRole("status")).toHaveTextContent(/Asking 3 servers/);
    const rows = within(dialog).getAllByText("Waiting");
    expect(rows).toHaveLength(3);
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(bridge.cancelled.length).toBeGreaterThan(0));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    slow.resolve({ profile: "research", servers: [] });
  });
});
