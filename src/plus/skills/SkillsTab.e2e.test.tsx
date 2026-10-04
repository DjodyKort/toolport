import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { openLibrary, write } from "./e2e";
import { createBridge, wire, type Bridge } from "./testkit";

/** The Skills tab walked the way a person uses it, against a world that changes: a sync writes
 * the outputs and clears the drift, a clean removes them and the lockfile, an uninstall removes
 * the skill, an install adds one. Each test is named by the parity action it proves
 * (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge({ world: true });
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter((line) => !/^(commands|skills|sources)( |$)/.test(line));
  expect(stray, "the tab only runs its own commands").toEqual([]);
  expect(
    bridge.ran().some((line) => /--home|secret|--reveal|stdin|token/.test(line)),
  ).toBe(false);
});

const list = () => screen.getByRole("list", { name: "Skills" });
const items = () => within(list()).getAllByRole("listitem");
const rowButton = (name: string) =>
  within(list())
    .getAllByRole("button")
    .find((b) => within(b).queryByText(name, { selector: "b" }))!;
const card = (name: string) => within(screen.getByRole("group", { name }));
const stat = (name: string) => within(screen.getByRole("group", { name }));
const detail = (name: string) => screen.getByRole("region", { name: `Skill ${name}` });
const syncState = (name: string) =>
  within(screen.getByRole("list", { name: `Sync state of ${name}` }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);

const synced = (user: Awaited<ReturnType<typeof openLibrary>>) =>
  write(user, "Sync…", "Sync", {
    first: "Preview",
    done: "Wrote 33 skills and 2 rules to 2 clients",
  });

describe("Skills tab, end to end: Installed", () => {
  it("skills.list: the Library screen opens on the library with 35 items and the sources", async () => {
    await openLibrary();
    expect(screen.getByRole("tab", { name: "Skills" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(items()).toHaveLength(35);
    expect(stat("Library").getByText("35 items")).toBeVisible();
    expect(stat("Library").getByText("33 skills, 2 rules")).toBeVisible();
    expect(stat("Visible to Claude").getByText("31 of 33")).toBeVisible();
    expect(within(rowButton("incident-notes")).getByText("not visible")).toBeVisible();
    expect(within(rowButton("house-style")).getByText("rule")).toBeVisible();
    const sources = within(screen.getByRole("group", { name: "Source" }));
    expect(sources.getByRole("button", { name: /^Library\s*35$/ })).toBeVisible();
    expect(sources.getByRole("button", { name: /^All\s*47$/ })).toBeVisible();
    expect(bridge.count("skills ls")).toBe(1);
    expect(bridge.missing).toEqual([]);
  });

  it("skills.list: a skill is picked and read with the keyboard alone", async () => {
    const user = await openLibrary();
    rowButton("api-review").focus();
    await user.keyboard("{Enter}");
    expect(detail("api-review")).toBeVisible();
    expect(rowButton("api-review")).toHaveAttribute("aria-current", "true");
    expect(syncState("api-review")).toEqual(["Claude CodeIn sync", "CursorMissing"]);
    await user.tab();
    expect(rowButton("build-triage")).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(rowButton("build-triage")).toHaveAttribute("aria-current", "true");
    expect(detail("build-triage")).toBeVisible();
    const tabs = screen.getByRole("tablist", { name: "Skills sections" });
    within(tabs).getByRole("tab", { name: "Installed" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(within(tabs).getByRole("tab", { name: "Taps" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.keyboard("{Home}");
    expect(within(tabs).getByRole("tab", { name: "Installed" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("skills.list: says so when toolportctl cannot run, and recovers on Retry", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    const user = await openLibrary(undefined, false);
    const failed = (await screen.findByText("Couldn't list skills")).closest(
      '[role="alert"]',
    ) as HTMLElement;
    expect(failed).toHaveTextContent(/toolportctl could not be started/);
    expect(screen.queryByRole("list", { name: "Skills" })).toBeNull();
    down = false;
    await user.click(within(failed).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(items()).toHaveLength(35));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Sync…" })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    expect(await screen.findByRole("dialog", { name: "Sync skills" })).toBeVisible();
  });

  it("skills.status, skills.diff: drift and changes show before a sync and are gone after it", async () => {
    const user = await openLibrary();
    expect(await card("Drift").findByText(/1 output missing or changed/)).toBeVisible();
    expect(
      card("Drift").getByText(
        (_, el) =>
          el?.tagName === "LI" &&
          /api-review is missing for Cursor/.test(el.textContent ?? ""),
      ),
    ).toBeVisible();
    expect(
      await card("Changes since last sync").findByText("deploy-helper"),
    ).toBeVisible();
    expect(card("Changes since last sync").getByText("feature-spec")).toBeVisible();
    expect(card("Changes since last sync").getByText("33 unchanged")).toBeVisible();
    expect(stat("Needs a look").getByText(/drift since the last sync/)).toBeVisible();
    await synced(user);
    expect(
      await card("Drift").findByText(/All 35 synced skills still in place/),
    ).toBeVisible();
    expect(
      await card("Changes since last sync").findByText("No changes since the last sync"),
    ).toBeVisible();
    expect(syncState("api-review")).toEqual(["Claude CodeIn sync", "CursorIn sync"]);
  });

  it("skills.lint, skills.audit: the checks read, and the lint of one skill is by name", async () => {
    const user = await openLibrary();
    expect(await card("Lint").findByText("0 errors, 2 warnings, 1 note")).toBeVisible();
    expect(await card("Audit").findByText("0 high, 1 medium, 0 low")).toBeVisible();
    await user.click(rowButton("deploy-helper"));
    expect(
      await within(detail("deploy-helper")).findByText(/longer than 200 characters/),
    ).toBeVisible();
    expect(bridge.count("skills lint --name deploy-helper")).toBe(1);
    expect(
      bridge.ran().filter((line) => /^skills (sync|clean|resolve)/.test(line)),
    ).toEqual(["skills sync --dry-run", "skills resolve --dry-run"]);
  });

  it("skills.sync: previews the clients of the lock first and a sync writes them", async () => {
    const user = await openLibrary();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const form = await screen.findByRole("dialog");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Sync skills to 2 clients/ });
    expect(
      within(box).getByText("Write 33 skills and 2 rules to 2 clients"),
    ).toBeVisible();
    expect(
      within(box).getByText(/deploy-helper: cursor: 'allowed-tools' field not supported/),
    ).toBeVisible();
    expect(bridge.count("skills sync --client claude-code --client cursor")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText("Wrote 33 skills and 2 rules to 2 clients");
    expect(bridge.count("skills sync --client claude-code --client cursor")).toBe(1);
    await waitFor(() => expect(bridge.count("skills status")).toBe(2));
  });

  it("skills.clean: a typed clean removes the outputs and the lockfile, and the screen says so", async () => {
    const user = await openLibrary();
    await write(user, "Clean outputs…", "Remove", {
      typed: "clean skills",
      done: "Removed 67 synced skill files",
    });
    expect(await card("Drift").findByText(/Not synced yet/)).toBeVisible();
    expect(
      await card("Changes since last sync").findByText(
        "Never synced: every skill is new",
      ),
    ).toBeVisible();
    await waitFor(() =>
      expect(
        within(detail("api-review")).getByText(/Not synced to any client yet/),
      ).toBeVisible(),
    );
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const form = await screen.findByRole("dialog");
    const ticked = within(form)
      .getAllByRole("checkbox")
      .filter((box) => (box as HTMLInputElement).checked);
    expect(
      ticked.map((box) => (box as HTMLInputElement).labels?.[0]?.textContent),
    ).toEqual(["Claude Code"]);
    expect(
      within(form).getByText(/no sync has chosen clients yet|not.*chosen/i),
    ).toBeVisible();
  });

  it("skills.uninstall: a typed name removes the skill from the library and the lock", async () => {
    const user = await openLibrary();
    await user.click(rowButton("api-review"));
    await write(user, "Uninstall api-review", "Uninstall", {
      typed: "api-review",
      done: /^Removed the skill 'api-review' and its 1 output/,
    });
    await waitFor(() => expect(items()).toHaveLength(34));
    expect(screen.queryByText("api-review", { selector: "b" })).toBeNull();
    expect(stat("Library").getByText("34 items")).toBeVisible();
    expect(bridge.count("skills uninstall api-review --dry-run")).toBe(1);
    expect(bridge.count("skills uninstall api-review")).toBe(1);
  });

  it("skills.resolve: a collision is resolved with --migrate and is gone afterwards", async () => {
    const user = await openLibrary();
    expect(stat("Needs a look").getByText(/1 collision/)).toBeVisible();
    await write(user, "Resolve…", "Resolve", { done: /^Resolved 1 collision/ });
    await waitFor(() => expect(stat("Needs a look").queryByText(/collision/)).toBeNull());
    expect(bridge.count("skills resolve --migrate --dry-run")).toBe(1);
    expect(bridge.count("skills resolve --migrate")).toBe(1);
  });

  it("skills.add: a new skill shows up in the list after the preview and the confirmation", async () => {
    const user = await openLibrary();
    await user.click(screen.getByRole("button", { name: "New skill…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByRole("textbox"), "reviewer");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Create skill reviewer/ });
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText(/^Created the skill 'reviewer'/);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    await waitFor(() => expect(items()).toHaveLength(36));
    expect(rowButton("reviewer")).toBeVisible();
    expect(bridge.count("skills add reviewer --type skill")).toBe(1);
  });

  it("skills.sync, skills.clean, skills.resolve, skills.uninstall: a project write names the project, and the user-level reads do not move", async () => {
    const user = await openLibrary();
    await user.click(screen.getByRole("radio", { name: "one project" }));
    await user.type(screen.getByLabelText("Project folder"), "/fixture/proj");
    await write(user, "Sync…", "Sync", {
      first: "Preview",
      done: "Wrote 33 skills and 2 rules to 2 clients",
    });
    await write(user, "Clean outputs…", "Remove", {
      typed: "clean skills",
      done: /^Removed 1 synced skill file/,
    });
    await user.click(rowButton("api-review"));
    await write(user, "Uninstall api-review", "Uninstall", {
      typed: "api-review",
      done: /^Removed the skill 'api-review'/,
    });
    await write(user, "Resolve…", "Resolve", { done: /^Resolved 1 collision/ });
    const ran = bridge.ran();
    for (const line of [
      "skills sync --project --repo /fixture/proj --client claude-code --client cursor",
      "skills clean --project --repo /fixture/proj",
      "skills uninstall api-review --project --repo /fixture/proj",
      "skills resolve --migrate --project --repo /fixture/proj",
    ])
      expect(ran, line).toContain(line);
    expect(items()).toHaveLength(35);
    expect(card("Drift").getByText(/1 output missing or changed/)).toBeVisible();
  });

  it("skills.clean: Escape cancels the preview and Enter does not confirm a typed phrase", async () => {
    const user = await openLibrary();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    const box = await screen.findByRole("dialog");
    await within(box).findByRole("button", { name: "Remove" });
    const phrase = within(box).getByRole("textbox");
    await user.type(phrase, "clean skills{Enter}");
    expect(bridge.count("skills clean")).toBe(0);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count("skills clean")).toBe(0);
    expect(bridge.count("skills clean --dry-run")).toBe(1);
  });
});

describe("Skills tab, end to end: Taps", () => {
  const taps = () =>
    within(screen.getByRole("list", { name: "Taps" })).getAllByRole("listitem");

  it("skills.tap.ls: lists the taps with their clone state", async () => {
    await openLibrary("Taps");
    await screen.findByRole("list", { name: "Taps" });
    expect(taps().map((li) => li.textContent)).toEqual([
      expect.stringContaining("acme-skills"),
      expect.stringContaining("local-notes"),
    ]);
    expect(within(taps()[0]).getByText("cloned")).toBeVisible();
    expect(within(taps()[1]).getByText("clone missing")).toBeVisible();
  });

  it("skills.tap.add: the clone is previewed, then the new tap is in the list", async () => {
    const user = await openLibrary("Taps");
    await screen.findByRole("list", { name: "Taps" });
    await user.click(screen.getByRole("button", { name: "Add tap…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByLabelText("Repository"), "acme/tools");
    await user.type(within(form).getByLabelText(/^Name/), "tools");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Add tap tools/ });
    expect(within(box).getByText(/needs the network/)).toBeVisible();
    expect(bridge.count("skills tap add acme/tools --name tools")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Add tap" }));
    await screen.findByText(/^Added the tap 'tools'/);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    await waitFor(() => expect(taps()).toHaveLength(3));
    expect(within(taps()[2]).getByText("tools")).toBeVisible();
  });

  it("skills.tap.update: one tap is updated after a preview and the list is read again", async () => {
    const user = await openLibrary("Taps");
    await screen.findByRole("list", { name: "Taps" });
    await write(user, "Update acme-skills", "Update", {
      done: "Updated 1 tap",
    });
    expect(bridge.count("skills tap update acme-skills --dry-run")).toBe(1);
    expect(bridge.count("skills tap update acme-skills")).toBe(1);
    expect(bridge.count("skills tap ls")).toBe(2);
  });

  it("skills.tap.remove: a plain confirmation removes the tap, and its hits leave the search", async () => {
    const user = await openLibrary("Taps");
    await screen.findByRole("list", { name: "Taps" });
    await write(user, "Remove acme-skills", "Remove", {
      done: "Removed the tap 'acme-skills'",
    });
    await waitFor(() => expect(taps()).toHaveLength(1));
    await user.click(screen.getByRole("tab", { name: "Find and install" }));
    await user.type(screen.getByRole("searchbox", { name: "Search the taps" }), "review");
    await user.click(screen.getByRole("button", { name: "Search" }));
    const hits = within(await screen.findByRole("list", { name: "Search results" }));
    expect(hits.getAllByRole("listitem")).toHaveLength(1);
    expect(hits.queryByRole("button", { name: "Install code-review" })).toBeNull();
  });

  it("skills.tap.add: a URL that carries a credential is refused and never reaches a command line", async () => {
    const canary = "CANARY-tap-token-7c1e";
    const user = await openLibrary("Taps");
    await screen.findByRole("list", { name: "Taps" });
    await user.click(screen.getByRole("button", { name: "Add tap…" }));
    const form = await screen.findByRole("dialog");
    await user.type(
      within(form).getByLabelText("Repository"),
      `https://deploy:${canary}@git.example.test/acme/private.git`,
    );
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText(/carries a credential/)).toBeVisible();
    expect(within(form).queryByText(new RegExp(canary))).toBeNull();
    expect(bridge.ran().some((line) => line.startsWith("skills tap add"))).toBe(false);
    expect(JSON.stringify(invoke.mock.calls)).not.toContain(canary);
    expect(document.body.textContent).not.toContain(canary);
  });
});

describe("Skills tab, end to end: Find and install", () => {
  const search = async (user: Awaited<ReturnType<typeof openLibrary>>, query: string) => {
    await user.type(screen.getByRole("searchbox", { name: "Search the taps" }), query);
    await user.click(screen.getByRole("button", { name: "Search" }));
  };

  it("skills.search: hits come from the taps, an empty answer says so", async () => {
    const user = await openLibrary("Find and install");
    await search(user, "review");
    const hits = within(await screen.findByRole("list", { name: "Search results" }));
    expect(hits.getAllByRole("listitem")).toHaveLength(2);
    expect(hits.getByRole("button", { name: "Install code-review" })).toBeEnabled();
    expect(hits.getByRole("button", { name: "Install note-review" })).toBeDisabled();
    await user.clear(screen.getByRole("searchbox", { name: "Search the taps" }));
    await search(user, "nothing");
    expect(await screen.findByText(/Nothing matches/)).toBeVisible();
  });

  it("skills.install: the audit finding shows in the plan and the installed skill joins the library", async () => {
    const user = await openLibrary("Find and install");
    await search(user, "review");
    await user.click(await screen.findByRole("button", { name: "Install code-review" }));
    const box = await screen.findByRole("dialog", {
      name: /Install @acme\/skills\/code-review/,
    });
    expect(within(box).getByText(/medium: code-review: Suspicious: sudo/)).toBeVisible();
    expect(within(box).getByText(/Clone and register the tap acme-skills/)).toBeVisible();
    expect(bridge.count("skills install @acme/skills/code-review")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Install" }));
    await screen.findByText("Installed 1 skill from acme-skills");
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    await user.click(screen.getByRole("tab", { name: "Installed" }));
    await waitFor(() => expect(items()).toHaveLength(36));
    expect(rowButton("code-review")).toBeVisible();
  });

  it("skills.install: a high finding blocks it, and only a typed --no-audit gets past", async () => {
    const user = await openLibrary("Find and install");
    await user.type(screen.getByLabelText("Spec"), "@acme/risky");
    await user.click(screen.getByRole("button", { name: "Preview install" }));
    const blocked = await screen.findByRole("dialog", {
      name: /Install @acme\/risky is blocked/,
    });
    expect(within(blocked).getByRole("alert")).toHaveTextContent(
      "1 high-severity finding in @acme/risky",
    );
    expect(within(blocked).queryByRole("button", { name: "Install" })).toBeNull();
    expect(bridge.count("skills install @acme/risky")).toBe(0);
    await user.click(
      within(blocked).getByRole("button", { name: "Install without the audit…" }),
    );
    const box = await screen.findByRole("dialog", { name: /Install without the audit/ });
    const confirm = within(box).getByRole("button", { name: "Install without audit" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "install without audit");
    await user.click(confirm);
    await waitFor(() =>
      expect(bridge.count("skills install @acme/risky --no-audit")).toBe(1),
    );
    expect(bridge.count("skills install @acme/risky")).toBe(0);
  });
});

describe("Skills tab, end to end: Bundles and init", () => {
  it("skills.bundle: the exact command is shown, the plan lists the skills, then it packs", async () => {
    const user = await openLibrary("Bundles");
    await user.type(
      screen.getByPlaceholderText("api-review, deploy-helper"),
      "api-review, deploy-helper",
    );
    await user.type(screen.getByLabelText("Zip file"), "/fixture/out/team.zip");
    expect(
      screen.getByText(
        "toolportctl skills bundle --skills api-review,deploy-helper --output /fixture/out/team.zip",
        { selector: "code" },
      ),
    ).toBeVisible();
    await write(user, "Preview bundle", "Pack", {
      done: "Packed 2 skills (2 files) into a zip",
    });
    expect(
      bridge.count(
        "skills bundle --skills api-review,deploy-helper --output /fixture/out/team.zip --dry-run",
      ),
    ).toBe(1);
  });

  it("skills.unbundle: the file an unbundle overwrites is listed before anything is written", async () => {
    const user = await openLibrary("Bundles");
    await user.type(screen.getByLabelText("Bundle zip"), "/fixture/in/team.zip");
    await user.type(screen.getByLabelText("Extract into"), "/fixture/fresh");
    await user.click(screen.getByRole("button", { name: "Preview unbundle" }));
    const box = await screen.findByRole("dialog", { name: /Extract a skills bundle/ });
    expect(within(box).getByText("1 file will be overwritten")).toBeVisible();
    expect(
      bridge.count("skills unbundle /fixture/in/team.zip --path /fixture/fresh"),
    ).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Extract" }));
    await waitFor(() =>
      expect(
        bridge.count("skills unbundle /fixture/in/team.zip --path /fixture/fresh"),
      ).toBe(1),
    );
  });
});

describe("Skills tab, end to end: no repository yet", () => {
  it("skills.init: the empty state creates the repository, then offers the first skill", async () => {
    bridge = createBridge({ world: { repo: false } });
    wire({ invoke, listen }, bridge);
    const user = await openLibrary(undefined, false);
    await screen.findByText("No skills repository yet");
    await user.click(screen.getByRole("button", { name: "Create repository…" }));
    const form = await screen.findByRole("dialog", {
      name: /Create a skills repository/,
    });
    await user.type(within(form).getByLabelText("Folder"), "/fixture/new");
    await user.type(within(form).getByLabelText(/^Name/), "team");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", {
      name: /Create the skills repository/,
    });
    expect(within(box).getByText("/fixture/new/skills/")).toBeVisible();
    expect(bridge.count("skills init --path /fixture/new --name team")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText(/^Created the skills repository/);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    expect(await screen.findByText("No skills in your library yet")).toBeVisible();
    expect(screen.getByRole("button", { name: /Create your first skill/ })).toBeEnabled();
  });
});
