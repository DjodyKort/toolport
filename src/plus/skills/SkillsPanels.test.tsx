import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen, pick } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  pick: { open: vi.fn(), save: vi.fn() },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => pick);
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { SkillsTab } from "./SkillsTab";
import { cleanData, syncData, CLIENTS } from "./fixtures";
import { tapLs, tapUpdateData } from "./fixturesTaps";
import { createBridge, failure, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
  pick.open.mockReset();
  pick.save.mockReset();
});

async function section(name: string) {
  const user = userEvent.setup();
  render(<SkillsTab />);
  await screen.findByRole("list", { name: "Skills" });
  await user.click(screen.getByRole("tab", { name }));
  return user;
}
const dialog = (name: RegExp) => screen.findByRole("dialog", { name });

describe("Taps panel", () => {
  it("lists the taps with their clone state", async () => {
    await section("Taps");
    const items = within(await screen.findByRole("list", { name: "Taps" })).getAllByRole(
      "listitem",
    );
    expect(items).toHaveLength(2);
    expect(within(items[0]).getByText("cloned")).toBeInTheDocument();
    expect(within(items[1]).getByText("clone missing")).toBeInTheDocument();
  });

  it("offers the first tap when there is none, and a Retry when listing fails", async () => {
    bridge.set("skills tap ls", { taps: [], tapsRoot: "/fixture/data/taps" });
    await section("Taps");
    expect(await screen.findByText("No taps yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update all…" })).toBeDisabled();
  });

  it("shows the CLI's words when the tap list fails", async () => {
    bridge.set("skills tap ls", failure("skills", "cannot read the taps file"));
    await section("Taps");
    expect(await screen.findByText(/cannot read the taps file/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("checks the repository, previews the clone, and adds only after confirming", async () => {
    const user = await section("Taps");
    await user.click(await screen.findByRole("button", { name: "Add tap…" }));
    const form = await dialog(/Add a tap/);
    await user.type(within(form).getByLabelText("Repository"), "not a repo");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByText(/Use user\/repo/)).toBeInTheDocument();
    await user.clear(within(form).getByLabelText("Repository"));
    await user.type(within(form).getByLabelText("Repository"), "acme/tools");
    await user.type(within(form).getByLabelText(/^Name/), "tools");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await dialog(/Add tap tools/);
    expect(
      within(box).getByText("Add the tap 'tools' from https://github.com/acme/tools.git"),
    ).toBeInTheDocument();
    expect(within(box).getByText(/needs the network/)).toBeInTheDocument();
    expect(bridge.count("skills tap add acme/tools --name tools --dry-run")).toBe(1);
    expect(bridge.count("skills tap add acme/tools --name tools")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Add tap" }));
    await screen.findByText(/^Added the tap 'tools'/);
    expect(bridge.count("skills tap add acme/tools --name tools")).toBe(1);
    await waitFor(() => expect(bridge.count("skills tap ls")).toBe(2));
  });

  it("updates one tap after a preview", async () => {
    const user = await section("Taps");
    await user.click(await screen.findByRole("button", { name: "Update acme-skills" }));
    const box = await dialog(/Update tap acme-skills/);
    expect(bridge.count("skills tap update acme-skills --dry-run")).toBe(1);
    expect(bridge.count("skills tap update acme-skills")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Update" }));
    await waitFor(() => expect(bridge.count("skills tap update acme-skills")).toBe(1));
  });

  it("names the tap that cannot be updated in the plan", async () => {
    bridge.set("skills tap update --dry-run", tapUpdateData(true, true));
    const user = await section("Taps");
    await user.click(await screen.findByRole("button", { name: "Update all…" }));
    const box = await dialog(/Update all taps/);
    expect(
      within(box).getByText("local-notes: could not reach the remote"),
    ).toBeInTheDocument();
  });

  it("removes a tap with a plain confirmation, because its tier is write", async () => {
    const user = await section("Taps");
    await user.click(await screen.findByRole("button", { name: "Remove acme-skills" }));
    const box = await dialog(/Remove tap acme-skills/);
    expect(within(box).getByText("Remove the tap 'acme-skills'")).toBeInTheDocument();
    expect(within(box).getByText("Delete the local clone")).toBeInTheDocument();
    expect(within(box).queryByRole("textbox")).toBeNull();
    expect(bridge.count("skills tap remove acme-skills")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(bridge.count("skills tap remove acme-skills")).toBe(1));
  });

  it("never applies when the preview of a tap write fails", async () => {
    bridge.set(
      "skills tap remove acme-skills --dry-run",
      failure("skills", "Tap not found."),
    );
    const user = await section("Taps");
    await user.click(await screen.findByRole("button", { name: "Remove acme-skills" }));
    expect(await screen.findByText("Tap not found.")).toBeInTheDocument();
    expect(bridge.count("skills tap remove acme-skills")).toBe(0);
  });
});

describe("Find and install panel", () => {
  async function search(user: ReturnType<typeof userEvent.setup>, query: string) {
    await user.type(screen.getByRole("searchbox", { name: "Search the taps" }), query);
    await user.click(screen.getByRole("button", { name: "Search" }));
  }

  it("lists the hits and derives the spec only for a GitHub tap", async () => {
    const user = await section("Find and install");
    await search(user, "review");
    const hits = within(await screen.findByRole("list", { name: "Search results" }));
    expect(hits.getAllByRole("listitem")).toHaveLength(2);
    expect(hits.getByRole("button", { name: "Install code-review" })).toBeEnabled();
    const local = hits.getByRole("button", { name: "Install note-review" });
    expect(local).toBeDisabled();
    expect(local).toHaveAttribute(
      "title",
      expect.stringContaining("not a GitHub repository"),
    );
  });

  it("says so when nothing matches and no tap exists", async () => {
    const user = await section("Find and install");
    await search(user, "nothing");
    expect(await screen.findByText(/Nothing matches/)).toBeInTheDocument();
    expect(screen.getByText(/You have no taps yet/)).toBeInTheDocument();
  });

  it("previews the install with its audit findings and installs after confirming", async () => {
    const user = await section("Find and install");
    await search(user, "review");
    await user.click(await screen.findByRole("button", { name: "Install code-review" }));
    const box = await dialog(/Install @acme\/skills\/code-review/);
    expect(within(box).getByText("Install 1 skill from acme-skills")).toBeInTheDocument();
    expect(
      within(box).getByText(/medium: code-review: Suspicious: sudo/),
    ).toBeInTheDocument();
    expect(
      within(box).getByText(/Clone and register the tap acme-skills/),
    ).toBeInTheDocument();
    expect(bridge.count("skills install @acme/skills/code-review --dry-run")).toBe(1);
    expect(bridge.count("skills install @acme/skills/code-review")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Install" }));
    await screen.findByText("Installed 1 skill from acme-skills");
    expect(bridge.count("skills install @acme/skills/code-review")).toBe(1);
  });

  it("blocks a high-severity finding and only --no-audit gets past it, typed", async () => {
    const user = await section("Find and install");
    await user.type(screen.getByLabelText("Spec"), "@acme/risky");
    await user.click(screen.getByRole("button", { name: "Preview install" }));
    const blocked = await dialog(/Install @acme\/risky is blocked/);
    expect(within(blocked).getByRole("alert")).toHaveTextContent(
      "1 high-severity finding in @acme/risky",
    );
    expect(within(blocked).getByText(/pipe a download into a shell/)).toBeInTheDocument();
    expect(within(blocked).queryByRole("button", { name: "Install" })).toBeNull();
    expect(bridge.count("skills install @acme/risky")).toBe(0);
    await user.click(
      within(blocked).getByRole("button", { name: "Install without the audit…" }),
    );
    const box = await dialog(/Install without the audit/);
    expect(within(box).getByText(/The audit was skipped/)).toBeInTheDocument();
    expect(bridge.count("skills install @acme/risky --no-audit --dry-run")).toBe(1);
    const confirm = within(box).getByRole("button", { name: "Install without audit" });
    expect(confirm).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "install without audit");
    await user.click(confirm);
    await waitFor(() =>
      expect(bridge.count("skills install @acme/risky --no-audit")).toBe(1),
    );
    expect(bridge.count("skills install @acme/risky")).toBe(0);
  });

  it("checks the spec before running anything and shows the exact command", async () => {
    const user = await section("Find and install");
    await user.type(screen.getByLabelText("Spec"), "risky");
    await user.click(screen.getByRole("button", { name: "Preview install" }));
    expect(screen.getByText(/Use @user\/repo/)).toBeInTheDocument();
    expect(bridge.ran().some((line) => line.startsWith("skills install"))).toBe(false);
    await user.clear(screen.getByLabelText("Spec"));
    await user.type(screen.getByLabelText("Spec"), "@acme/skills/code-review");
    expect(
      screen.getByText("toolportctl skills install @acme/skills/code-review", {
        selector: "code",
      }),
    ).toBeInTheDocument();
  });
});

describe("Bundles panel", () => {
  it("previews a bundle, shows the command, and packs only after confirming", async () => {
    const user = await section("Bundles");
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
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Preview bundle" }));
    const box = await dialog(/Pack skills into a zip/);
    expect(
      within(box).getByText("Pack 2 skills (2 files) into a zip"),
    ).toBeInTheDocument();
    expect(
      bridge.count(
        "skills bundle --skills api-review,deploy-helper --output /fixture/out/team.zip",
      ),
    ).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Pack" }));
    await screen.findByText("Packed 2 skills (2 files) into a zip");
  });

  it("fills a path from the native picker when there is one, and says when there is not", async () => {
    pick.save.mockResolvedValueOnce("/fixture/out/team.zip");
    const user = await section("Bundles");
    await user.click(screen.getByRole("button", { name: "Choose zip file" }));
    await waitFor(() =>
      expect(screen.getByLabelText("Zip file")).toHaveValue("/fixture/out/team.zip"),
    );
    pick.open.mockRejectedValueOnce(new Error("no dialog"));
    await user.click(screen.getByRole("button", { name: "Choose bundle zip" }));
    expect(await screen.findByText(/picker is not available here/)).toBeInTheDocument();
  });

  it("lists the files an unbundle overwrites before it writes", async () => {
    const user = await section("Bundles");
    await user.click(screen.getByRole("button", { name: "Preview unbundle" }));
    expect(screen.getByText("Give the zip file to extract")).toBeInTheDocument();
    expect(bridge.ran().some((line) => line.startsWith("skills unbundle"))).toBe(false);
    await user.type(screen.getByLabelText("Bundle zip"), "/fixture/in/team.zip");
    await user.type(screen.getByLabelText("Extract into"), "/fixture/fresh");
    await user.click(screen.getByRole("button", { name: "Preview unbundle" }));
    const box = await dialog(/Extract a skills bundle/);
    expect(within(box).getByText("1 file will be overwritten")).toBeInTheDocument();
    expect(
      within(box).getByText("/fixture/fresh/skills/deploy-helper/SKILL.md"),
    ).toBeInTheDocument();
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

describe("Repository, scope and tools", () => {
  it("offers to create the repository when there is none", async () => {
    bridge.set("skills ls", failure("skills", "no skills repository at /fixture/new"));
    const user = userEvent.setup();
    render(<SkillsTab />);
    await screen.findByText("No skills repository yet");
    await user.click(screen.getByRole("button", { name: "Create repository…" }));
    const form = await dialog(/Create a skills repository/);
    await user.type(within(form).getByLabelText("Folder"), "/fixture/new");
    await user.type(within(form).getByLabelText(/^Name/), "team");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    const box = await dialog(/Create the skills repository/);
    expect(
      within(box).getByText("Create the skills repository 'team' at /fixture/new"),
    ).toBeInTheDocument();
    expect(within(box).getByText("/fixture/new/skills/")).toBeInTheDocument();
    expect(bridge.count("skills init --path /fixture/new --name team")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Create" }));
    await waitFor(() =>
      expect(bridge.count("skills init --path /fixture/new --name team")).toBe(1),
    );
  });

  it("writes to one project only when asked: --project with its folder", async () => {
    bridge.set("skills clean --project --repo /fixture/proj --dry-run", cleanData(true));
    bridge.set("skills clean --project --repo /fixture/proj", cleanData(false));
    bridge.set(
      "skills sync --project --repo /fixture/proj --client claude-code --client cursor --dry-run",
      syncData(CLIENTS, true),
    );
    const user = await section("Installed");
    await screen.findByRole("list", { name: "Skills" });
    await user.click(screen.getByRole("radio", { name: "one project" }));
    await user.type(screen.getByLabelText("Project folder"), "/fixture/proj");
    await user.click(screen.getByRole("button", { name: "Sync…" }));
    await user.click(
      within(await dialog(/Sync skills/)).getByRole("button", { name: "Preview" }),
    );
    await dialog(/Sync skills to 2 clients/);
    expect(
      bridge.count(
        "skills sync --project --repo /fixture/proj --client claude-code --client cursor --dry-run",
      ),
    ).toBe(1);
    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(screen.getByRole("button", { name: "Clean outputs…" }));
    const box = await dialog(/Remove synced skill files/);
    await user.type(within(box).getByRole("textbox"), "clean skills");
    await user.click(within(box).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(bridge.count("skills clean --project --repo /fixture/proj")).toBe(1),
    );
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
  });

  it("keeps the self-MCP tools without a CLI twin off, each with its reason", async () => {
    await section("Installed");
    await screen.findByRole("list", { name: "Actions not available yet" });
    for (const name of ["Scaffold with progressive files"]) {
      const button = screen.getByRole("button", { name });
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("title", expect.stringContaining("MIG-GUI-14"));
    }
  });

  it("runs no command the fake bridge has no reply for", async () => {
    const user = await section("Taps");
    await screen.findByRole("list", { name: "Taps" });
    await user.click(screen.getByRole("tab", { name: "Bundles" }));
    expect(bridge.missing).toEqual([]);
    void tapLs;
  });
});
