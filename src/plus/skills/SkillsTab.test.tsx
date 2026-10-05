import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { SkillsTab } from "./SkillsTab";
import { libraryLs, statusData, syncData } from "./fixtures";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

async function open() {
  const user = userEvent.setup();
  render(<SkillsTab />);
  await screen.findByRole("list", { name: "Skills" });
  return user;
}

const list = () => screen.getByRole("list", { name: "Skills" });
const rowButton = (name: string) =>
  within(list())
    .getAllByRole("button")
    .find((b) => within(b).queryByText(name, { selector: "b" }))!;
const detail = (name: string) => screen.getByRole("region", { name: `Skill ${name}` });
const chips = () => within(screen.getByRole("group", { name: "Source" }));
const dialog = () => screen.findByRole("dialog");

describe("Skills tab: reading", () => {
  it("shows the library with its numbers: 35 items, 33 skills and 2 rules", async () => {
    await open();
    const stat = (label: string) => within(screen.getByRole("group", { name: label }));
    expect(stat("Library").getByText("35 items")).toBeInTheDocument();
    expect(stat("Library").getByText("33 skills, 2 rules")).toBeInTheDocument();
    expect(within(list()).getAllByRole("listitem")).toHaveLength(35);
    expect(stat("Visible to Claude").getByText("31 of 33")).toBeInTheDocument();
    expect(stat("Needs a look").getByText(/1 collision/)).toBeInTheDocument();
    expect(
      within(rowButton("incident-notes")).getByText("not visible"),
    ).toBeInTheDocument();
    expect(within(rowButton("house-style")).getByText("rule")).toBeInTheDocument();
  });

  it("puts a source badge on every row and a filter chip on every source", async () => {
    await open();
    expect(
      within(rowButton("api-review")).getByText("library", { selector: "span" }),
    ).toBeInTheDocument();
    expect(chips().getByRole("button", { name: /^All\s*47$/ })).toBeInTheDocument();
    expect(chips().getByRole("button", { name: /^Library\s*35$/ })).toBeInTheDocument();
    expect(chips().getByRole("button", { name: /^odh\s*3$/ })).toBeInTheDocument();
    expect(
      chips().getByRole("button", { name: /^alpha@market\s*1$/ }),
    ).toBeInTheDocument();
  });

  it("lists one source with skills ls --source and marks it read-only", async () => {
    const user = await open();
    await user.click(chips().getByRole("button", { name: /^odh/ }));
    await waitFor(() => expect(within(list()).getAllByRole("listitem")).toHaveLength(3));
    expect(bridge.count("skills ls --source repo:odh")).toBe(1);
    expect(within(list()).getAllByText("repo", { selector: "span" })).toHaveLength(3);
    const first = screen.getByRole("region", { name: /^Skill repo-odh-skill-1/ });
    expect(within(first).getByText("read-only")).toBeInTheDocument();
    expect(within(first).getByText(/never edits it/)).toBeInTheDocument();
    expect(within(first).queryByRole("button", { name: /Uninstall/ })).toBeNull();
    expect(within(first).queryByText("Lint")).toBeNull();
  });

  it("says a source stopped early instead of showing a silent partial list", async () => {
    const user = await open();
    await user.click(chips().getByRole("button", { name: /^repos\/core/ }));
    expect(
      await screen.findByText(/is incomplete: stopped at the 2 s budget/),
    ).toBeInTheDocument();
  });

  it("merges every source under All and names a source that failed", async () => {
    bridge.set(
      "skills ls --source tap:acme-tools",
      failure("skills", "tap clone is missing"),
    );
    const user = await open();
    await user.click(chips().getByRole("button", { name: /^All/ }));
    expect(
      await screen.findByText(/tap:acme-tools could not be read: tap clone is missing/),
    ).toBeInTheDocument();
    expect(within(list()).getAllByRole("listitem")).toHaveLength(46);
  });

  it("shows the sync state per client and the drift of one skill", async () => {
    const user = await open();
    const states = within(
      screen.getByRole("list", { name: "Sync state of api-review" }),
    ).getAllByRole("listitem");
    expect(states.map((li) => li.textContent)).toEqual([
      "Claude CodeIn sync",
      "CursorMissing",
    ]);
    await user.click(rowButton("deploy-helper"));
    const box = detail("deploy-helper");
    const drifted = within(
      within(box).getByRole("list", { name: "Sync state of deploy-helper" }),
    ).getAllByText("Changed since sync");
    expect(drifted).toHaveLength(2);
  });

  it("lints the selected skill by name and lists its audit findings", async () => {
    const user = await open();
    await user.click(rowButton("deploy-helper"));
    const box = detail("deploy-helper");
    expect(
      await within(box).findByText(/longer than 200 characters/),
    ).toBeInTheDocument();
    expect(bridge.count("skills lint --name deploy-helper")).toBe(1);
    expect(
      within(box).getByText(/sudo usage in skill instructions \(line 3\)/),
    ).toBeInTheDocument();
  });

  it("explains why a skill is invisible and what Claude Code rejected", async () => {
    const user = await open();
    await user.click(rowButton("incident-notes"));
    const box = detail("incident-notes");
    expect(within(box).getByText("no: slash command only")).toBeInTheDocument();
    expect(within(box).getByText(/Rejects the Claude Code copy/)).toBeInTheDocument();
  });

  it("links an invisible skill to the lint output", async () => {
    const user = await open();
    await user.click(rowButton("incident-notes"));
    const box = detail("incident-notes");
    await user.click(within(box).getByRole("button", { name: "See the lint output" }));
    expect(screen.getByRole("group", { name: "Lint" })).toHaveFocus();
    await user.click(rowButton("api-review"));
    expect(
      within(detail("api-review")).queryByRole("button", { name: "See the lint output" }),
    ).toBeNull();
  });

  it("shows the checks: lint, audit, changes since the last sync and drift", async () => {
    await open();
    const card = (name: string) => within(screen.getByRole("group", { name }));
    expect(
      await card("Lint").findByText(
        "2 errors, 0 warnings, 1 note".replace("2 errors, 0", "0 errors, 2"),
      ),
    ).toBeInTheDocument();
    expect(await card("Audit").findByText("0 high, 1 medium, 0 low")).toBeInTheDocument();
    expect(
      await card("Changes since last sync").findByText("deploy-helper"),
    ).toBeInTheDocument();
    expect(card("Changes since last sync").getByText("33 unchanged")).toBeInTheDocument();
    expect(
      await card("Drift").findByText(/1 output missing or changed/),
    ).toBeInTheDocument();
  });

  it("offers to create the first skill when the library is empty", async () => {
    bridge.set("skills ls", { repo: "/fixture/skills-repo", skills: [] });
    render(<SkillsTab />);
    expect(await screen.findByText("No skills in your library yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Create your first skill/ })).toBeEnabled();
  });

  it("shows the CLI's words and a Retry when the list fails", async () => {
    bridge.set("skills ls", failure("skills", "cannot read /fixture/skills-repo: busy"));
    render(<SkillsTab />);
    expect(
      await screen.findByText(/cannot read \/fixture\/skills-repo/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("opens the editor for a library skill", async () => {
    await open();
    expect(
      screen.getByRole("button", { name: "Open editor for api-review" }),
    ).toBeEnabled();
  });

  it("never reads with --home and runs no command it has no reply for", async () => {
    await open();
    await screen.findByText("0 high, 1 medium, 0 low");
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
    expect(bridge.missing).toEqual([]);
  });
});

describe("Skills tab: writing", () => {
  it("offers only the clients of the last sync, never all of them", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const box = await dialog();
    const boxes = within(box).getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes).toHaveLength(15);
    expect(boxes.filter((c) => c.checked).map((c) => c.labels?.[0]?.textContent)).toEqual(
      ["Claude Code", "Cursor"],
    );
    await user.click(within(box).getByRole("checkbox", { name: "Claude Code" }));
    await user.click(within(box).getByRole("checkbox", { name: "Cursor" }));
    expect(within(box).getByText("Pick at least one client")).toBeInTheDocument();
    expect(within(box).getByRole("button", { name: "Preview" })).toBeDisabled();
    expect(
      bridge.ran().filter((line) => line.startsWith("skills sync --client")),
    ).toEqual([]);
  });

  it("previews the sync of the chosen clients first and writes only after Sync", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    const form = await dialog();
    await user.click(within(form).getByRole("checkbox", { name: "Cursor" }));
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Sync skills to 1 client/ });
    expect(
      within(box).getByText("Write 33 skills and 2 rules to 1 client"),
    ).toBeInTheDocument();
    expect(
      within(box).getByText(
        /release-notes \(Claude Code\): the command file shadows the skill/,
      ),
    ).toBeInTheDocument();
    expect(
      within(box).getByText("toolportctl skills sync --client claude-code"),
    ).toBeInTheDocument();
    expect(bridge.count("skills sync --client claude-code --dry-run")).toBe(1);
    expect(bridge.count("skills sync --client claude-code")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Sync" }));
    await screen.findByText("Wrote 33 skills and 2 rules to 1 client");
    expect(bridge.count("skills sync --client claude-code")).toBe(1);
    await waitFor(() => expect(bridge.count("skills status")).toBe(2));
  });

  it("shows the fields a client drops as warnings of the plan", async () => {
    bridge.set(
      "skills sync --client claude-code --client cursor --dry-run",
      syncData(["claude-code", "cursor"], true),
    );
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    await user.click(within(await dialog()).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Sync skills to 2 clients/ });
    expect(
      within(box).getByText(/deploy-helper: cursor: 'allowed-tools' field not supported/),
    ).toBeInTheDocument();
  });

  it("makes you type the phrase before cleaning, and previews the files first", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    const box = await dialog();
    expect(within(box).getByText("Remove 1 synced skill file")).toBeInTheDocument();
    expect(bridge.count("skills clean --dry-run")).toBe(1);
    const confirm = within(box).getByRole("button", { name: "Remove" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "clean skills");
    await user.click(confirm);
    await screen.findByText("Removed 1 synced skill file");
    expect(bridge.count("skills clean")).toBe(1);
  });

  it("uninstalls a library skill after typing its name", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Uninstall api-review" }));
    const box = await dialog();
    expect(
      within(box).getByText("Remove the skill 'api-review' and its 2 outputs"),
    ).toBeInTheDocument();
    expect(bridge.count("skills uninstall api-review")).toBe(0);
    const confirm = within(box).getByRole("button", { name: "Uninstall" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "api-review");
    await user.click(confirm);
    await waitFor(() => expect(bridge.count("skills uninstall api-review")).toBe(1));
  });

  it("resolves a collision with --migrate after a preview of what moves", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Resolve…" }));
    const box = await dialog();
    expect(within(box).getByText("Resolve 1 collision")).toBeInTheDocument();
    expect(within(box).getByText(/the command file is moved to/)).toBeInTheDocument();
    expect(bridge.count("skills resolve --migrate")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Resolve" }));
    await waitFor(() => expect(bridge.count("skills resolve --migrate")).toBe(1));
  });

  it("creates a skill from the template: name check, preview, confirm", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: "New skill…" }));
    const form = await dialog();
    await user.type(within(form).getByRole("textbox"), "Bad Name");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText(/lowercase/)).toBeInTheDocument();
    await user.clear(within(form).getByRole("textbox"));
    await user.type(within(form).getByRole("textbox"), "reviewer");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await screen.findByRole("dialog", { name: /Create skill reviewer/ });
    expect(within(box).getByText(/Create the skill 'reviewer'/)).toBeInTheDocument();
    expect(bridge.count("skills add reviewer --type skill")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await screen.findByText(/^Created the skill 'reviewer'/);
    expect(bridge.count("skills add reviewer --type skill")).toBe(1);
  });

  it("never applies when the preview failed", async () => {
    bridge.set("skills clean --dry-run", failure("failed", "cannot read the lockfile"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    expect(await screen.findByText("cannot read the lockfile")).toBeInTheDocument();
    expect(bridge.count("skills clean")).toBe(0);
  });

  it("refuses a write the registry does not classify", async () => {
    bridge.set("commands", { commands: [], tools: [] });
    const user = await open();
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    expect(
      await screen.findByText(/does not know how safe `skills clean` is/),
    ).toBeInTheDocument();
    expect(bridge.count("skills clean --dry-run")).toBe(0);
  });
});

void libraryLs;
void statusData;
