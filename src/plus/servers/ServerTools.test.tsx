import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { error: vi.fn(), success: vi.fn() }),
}));

import { commandsGolden } from "../fixtures/servers";
import { renderScreen, wire } from "./harness";
import { NO_MCP_CALL } from "./mcpTools";
import {
  bridgeDown,
  createBridge,
  ctlFailure,
  deferred,
  type Bridge,
  type Call,
} from "./testkit";
import { TOOL_NAMES, createToolsWorld, type ToolsWorld } from "./world";

let bridge: Bridge;
let world: ToolsWorld;

function attach(target: ToolsWorld) {
  for (const tool of TOOL_NAMES) {
    bridge.set(`mcp call ${tool} --args-stdin`, (_argv: string[], call: Call) =>
      target.reply(call.argv, call.stdin),
    );
  }
  bridge.set("profile ls", () => target.profileLs());
}

beforeEach(() => {
  bridge = createBridge();
  wire(mocks, bridge);
  world = createToolsWorld();
  attach(world);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
});

const toolCalls = () =>
  bridge.calls
    .filter((call) => call.argv[0] === "mcp")
    .map((call) => ({
      tool: call.argv[2],
      args: call.stdin ? (JSON.parse(call.stdin) as Record<string, unknown>) : null,
    }));
const callsOf = (tool: string) => toolCalls().filter((call) => call.tool === tool);

type User = ReturnType<typeof renderScreen>["user"];

const closeOf = (dialog: HTMLElement) =>
  within(dialog)
    .getAllByRole("button", { name: "Close" })
    .find((button) => button.dataset.slot !== "dialog-close")!;

async function openServer(user: User, name: string) {
  const list = await screen.findByRole("region", { name: "Connected" });
  await user.click(within(list).getByRole("button", { name: new RegExp(`${name} `) }));
  const detail = await screen.findByRole("region", { name: `${name} details` });
  const tools = await within(detail).findByRole("region", {
    name: "Source, updates and mode",
  });
  return { detail, tools: within(tools) };
}

async function openApply(user: User, tools: ReturnType<typeof within>) {
  await user.click(tools.getByRole("button", { name: "Check for updates" }));
  await user.click(await tools.findByRole("button", { name: "Update docs-search" }));
  return screen.findByRole("dialog", { name: "Update docs-search?" });
}

describe("servers.detect-source: where a server came from", () => {
  it("reads only when asked, with the arguments on stdin and none in argv", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    expect(toolCalls()).toEqual([]);

    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText("git")).toBeInTheDocument();
    expect(tools.getByText("stored in the registry")).toBeInTheDocument();
    expect(tools.getByText("/fixture/home/src/docs-search")).toBeInTheDocument();
    expect(tools.getByText(/on branch main/)).toBeInTheDocument();
    expect(callsOf("servers_detect_source")).toEqual([
      { tool: "servers_detect_source", args: { name: "docs-search" } },
    ]);
    const argv = bridge.calls.find((call) => call.argv[2] === "servers_detect_source");
    expect(argv?.argv).toEqual(["mcp", "call", "servers_detect_source", "--args-stdin"]);
  });

  it("e2e: says a source was only detected, and has no git panel for it", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "mail-bridge");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText("Unknown source")).toBeInTheDocument();
    expect(tools.getByText("detected, not stored yet")).toBeInTheDocument();
    expect(tools.queryByRole("button", { name: "Sync with upstream" })).toBeNull();
    expect(callsOf("servers_git_status")).toEqual([]);
    await user.click(tools.getByRole("button", { name: "Detect again" }));
    await waitFor(() => expect(callsOf("servers_detect_source")).toHaveLength(2));
  });

  it("shows the tool's own error in words, and recovers on the next try", async () => {
    bridge.set("mcp call servers_detect_source --args-stdin", () =>
      ctlFailure("not_found", "server not found: docs-search"),
    );
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByRole("alert")).toHaveTextContent(
      "server not found: docs-search",
    );
    attach(world);
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText("git")).toBeInTheDocument();
    expect(tools.queryByRole("alert")).toBeNull();
  });

  it("says toolportctl could not be run when the bridge is down", async () => {
    bridge.set("mcp call servers_detect_source --args-stdin", bridgeDown("spawn failed"));
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByRole("alert")).toHaveTextContent(/spawn failed/);
  });

  it("shows a skeleton while the read is on its way", async () => {
    const slow = deferred<unknown>();
    bridge.set("mcp call servers_detect_source --args-stdin", () => slow.promise);
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByLabelText("Looking for the source")).toBeInTheDocument();
    expect(
      tools.getByRole("button", { name: "Where did this come from?" }),
    ).toBeDisabled();
    slow.resolve(world.call("servers_detect_source", { name: "docs-search" }));
    expect(await tools.findByText("git")).toBeInTheDocument();
  });
});

describe("servers.git-status: the git state of a checkout", () => {
  it("e2e: reads it once the source is a git checkout and shows what upstream has", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText("tracks origin/main")).toBeInTheDocument();
    expect(tools.getByText("1 ahead")).toBeInTheDocument();
    expect(tools.getByText("2 behind")).toBeInTheDocument();
    expect(tools.getByText("clean")).toBeInTheDocument();
    const commits = tools.getByRole("list", { name: "Upstream commits" });
    expect(within(commits).getByText("6a4a877 upstream change")).toBeInTheDocument();
    expect(callsOf("servers_git_status")).toEqual([
      { tool: "servers_git_status", args: { name: "docs-search" } },
    ]);

    world.servers[0].git!.behind = 0;
    world.servers[0].git!.summaries = [];
    await user.click(tools.getByRole("button", { name: "Refresh git state" }));
    expect(await tools.findByText("0 behind")).toBeInTheDocument();
    expect(tools.queryByRole("list", { name: "Upstream commits" })).toBeNull();
  });

  it("holds the sync back, with the reason, while the checkout has changes", async () => {
    world.servers[0].git!.dirty = true;
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText("uncommitted changes")).toBeInTheDocument();
    expect(tools.getByRole("status")).toHaveTextContent(/Commit or stash them first/);
    expect(tools.getByRole("button", { name: "Sync with upstream" })).toBeDisabled();
  });

  it("shows a tool error of the git read instead of an empty panel", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    bridge.set("mcp call servers_git_status --args-stdin", () =>
      ctlFailure("backend_error", "git is not installed"),
    );
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    expect(await tools.findByText(/git is not installed/)).toBeInTheDocument();
  });
});

describe("servers.check-updates: checking one server", () => {
  it("e2e: shows what is available and the update command that would run", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Check for updates" }));
    expect(await tools.findByText("Update available")).toBeInTheDocument();
    expect(tools.getByText(/2 commits behind origin\/main/)).toBeInTheDocument();
    expect(tools.getByText("npm run build")).toBeInTheDocument();
    expect(tools.getByText(/runs when you apply the update/)).toBeInTheDocument();
    expect(tools.queryByText(/not run without/)).toBeNull();
    expect(callsOf("servers_check_updates")).toEqual([
      { tool: "servers_check_updates", args: { name: "docs-search" } },
    ]);
    expect(tools.getByText(/also listed in System/)).toBeInTheDocument();
  });

  it("shows up to date without an Update button, and a skipped server with its reason", async () => {
    const { user } = renderScreen();
    const wiki = await openServer(user, "wiki-reader");
    await user.click(wiki.tools.getByRole("button", { name: "Check for updates" }));
    expect(await wiki.tools.findByText("Up to date")).toBeInTheDocument();
    expect(wiki.tools.queryByRole("button", { name: /^Update / })).toBeNull();

    const corp = await openServer(user, "corp-tools");
    await user.click(corp.tools.getByRole("button", { name: "Check for updates" }));
    expect(await corp.tools.findByText("Skipped")).toBeInTheDocument();
    expect(corp.tools.getByText(/remote server, nothing to update/)).toBeInTheDocument();
  });

  it("keeps the buttons off, with the reason, when this build has no mcp call", async () => {
    bridge.set("commands", {
      ...commandsGolden,
      commands: commandsGolden.commands.filter((row) => row.id !== "mcp call"),
    });
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    expect(tools.getByText(NO_MCP_CALL)).toBeInTheDocument();
    for (const name of ["Check for updates", "Change mode…", "Add to a profile…"]) {
      expect(tools.getByRole("button", { name })).toBeDisabled();
    }
    expect(toolCalls()).toEqual([]);
  });

  it("shows an error of the check and offers the check again", async () => {
    bridge.set("mcp call servers_check_updates --args-stdin", () =>
      ctlFailure("backend_error", "could not reach the host"),
    );
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Check for updates" }));
    expect(await tools.findByRole("alert")).toHaveTextContent("could not reach the host");
    expect(tools.getByRole("button", { name: "Check for updates" })).toBeEnabled();
  });
});

describe("servers.apply-update: applying an update", () => {
  it("e2e: says the update command runs, applies only after the confirmation, and resets the check", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    const confirm = await openApply(user, tools);
    expect(
      within(confirm).getByText(
        "Runs the stored update command (post_update): npm run build",
      ),
    ).toBeInTheDocument();
    expect(
      within(confirm).getByText(/also runs the server's stored update command/),
    ).toBeInTheDocument();
    expect(within(confirm).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl mcp call servers_apply_update --args-stdin",
    );
    expect(callsOf("servers_apply_update")).toEqual([]);

    await user.click(
      within(confirm).getByRole("button", { name: "Update and run its command" }),
    );
    const done = await screen.findByRole("dialog", { name: "Update docs-search" });
    expect(await within(done).findByText("docs-search: updated")).toBeInTheDocument();
    expect(callsOf("servers_apply_update")).toEqual([
      { tool: "servers_apply_update", args: { name: "docs-search", confirm: true } },
    ]);
    await user.click(closeOf(done));

    const again = (await openServer(user, "docs-search")).tools;
    expect(again.queryByText("Update available")).toBeNull();
    await user.click(again.getByRole("button", { name: "Check for updates" }));
    expect(await again.findByText("Up to date")).toBeInTheDocument();
  });

  it("applies nothing when you cancel, and gives focus back to the opener", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await openApply(user, tools);
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Update docs-search?" })).toBeNull(),
    );
    expect(callsOf("servers_apply_update")).toEqual([]);
    expect(tools.getByRole("button", { name: "Update docs-search" })).toHaveFocus();
  });

  it("shows a refusal of the tool with its words and changes nothing", async () => {
    bridge.set("mcp call servers_apply_update --args-stdin", () =>
      ctlFailure(
        "refused",
        "Refused: servers_apply_update (tier 3). Pass confirm=true to proceed.",
      ),
    );
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    const confirm = await openApply(user, tools);
    await user.click(
      within(confirm).getByRole("button", { name: "Update and run its command" }),
    );
    const done = await screen.findByRole("dialog", { name: "Update docs-search" });
    expect(
      await within(done).findByText(/Pass confirm=true to proceed/),
    ).toBeInTheDocument();
    expect(world.servers[0].update).toBe("update-available");
  });
});

describe("servers.fork-sync: syncing a checkout with its upstream", () => {
  async function toSyncDialog(user: User) {
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    await tools.findByText("tracks origin/main");
    await user.click(tools.getByRole("button", { name: "Sync with upstream" }));
    return {
      tools,
      form: await screen.findByRole("dialog", {
        name: "Sync docs-search with its upstream",
      }),
    };
  }

  it("e2e: previews the plan, applies on confirm and reads the new branch", async () => {
    const { user } = renderScreen();
    const { tools, form } = await toSyncDialog(user);
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream?",
    });
    expect(
      within(confirm).getByText(
        /Rebase main-synced-<date> onto upstream\/main|Rebase main-synced/,
      ),
    ).toBeInTheDocument();
    expect(
      within(confirm).getByText("Upstream: 6a4a877 upstream change"),
    ).toBeInTheDocument();
    expect(
      within(confirm).getByText(/A conflict stops the sync half way/),
    ).toBeInTheDocument();
    expect(callsOf("servers_fork_sync")).toEqual([]);

    await user.click(within(confirm).getByRole("button", { name: "Sync" }));
    const done = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream",
    });
    expect(
      await within(done).findByText("docs-search synced on main-synced-20261005"),
    ).toBeInTheDocument();
    expect(callsOf("servers_fork_sync")).toEqual([
      {
        tool: "servers_fork_sync",
        args: {
          name: "docs-search",
          upstream_remote: "upstream",
          upstream_branch: "main",
          mode: "rebase",
          confirm: true,
        },
      },
    ]);
    await user.click(closeOf(done));
    const after = (await openServer(user, "docs-search")).tools;
    await user.click(after.getByRole("button", { name: "Where did this come from?" }));
    expect(await after.findByText("0 behind")).toBeInTheDocument();
    expect(after.getByText("main-synced-20261005")).toBeInTheDocument();
    expect(tools).toBeTruthy();
  });

  it("asks for the author when only one author's commits are kept, and sends the options", async () => {
    const { user } = renderScreen();
    const { form } = await toSyncDialog(user);
    await user.selectOptions(within(form).getByLabelText("How"), "onto-author");
    await user.click(within(form).getByRole("button", { name: "Review" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent(/author email/);
    expect(callsOf("servers_fork_sync")).toEqual([]);

    await user.type(within(form).getByLabelText("Author email"), "me@example.test");
    await user.type(within(form).getByLabelText("New branch"), "main-synced");
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream?",
    });
    await user.click(within(confirm).getByRole("button", { name: "Sync" }));
    await waitFor(() => expect(callsOf("servers_fork_sync")).toHaveLength(1));
    expect(callsOf("servers_fork_sync")[0].args).toEqual({
      name: "docs-search",
      upstream_remote: "upstream",
      upstream_branch: "main",
      mode: "onto-author",
      author_email: "me@example.test",
      target_branch: "main-synced",
      confirm: true,
    });
  });

  it("offers to run the stored update command only when the server has one", async () => {
    const { user } = renderScreen();
    const { tools, form } = await toSyncDialog(user);
    expect(within(form).queryByRole("checkbox")).toBeNull();
    await user.keyboard("{Escape}");
    await user.click(tools.getByRole("button", { name: "Check for updates" }));
    await tools.findByText("Update available");
    await user.click(tools.getByRole("button", { name: "Sync with upstream" }));
    const again = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream",
    });
    await user.click(
      within(again).getByRole("checkbox", { name: /stored update command/ }),
    );
    await user.click(within(again).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream?",
    });
    expect(
      within(confirm).getByText(
        /Runs the stored update command \(post_update\) when the sync worked/,
      ),
    ).toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "Sync" }));
    await waitFor(() => expect(callsOf("servers_fork_sync")).toHaveLength(1));
    expect(callsOf("servers_fork_sync")[0].args?.run_post_update).toBe(true);
  });

  it("shows a conflict as a stopped sync with the files, not as a success", async () => {
    world.conflictNext();
    const { user } = renderScreen();
    const { form } = await toSyncDialog(user);
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream?",
    });
    await user.click(within(confirm).getByRole("button", { name: "Sync" }));
    const done = await screen.findByRole("dialog", {
      name: "Sync docs-search with its upstream",
    });
    expect(
      await within(done).findByText("The sync of docs-search stopped on a conflict"),
    ).toBeInTheDocument();
    expect(within(done).getByText("src/index.ts")).toBeInTheDocument();
    expect(within(done).getByText(/git rebase --continue/)).toBeInTheDocument();
  });

  it("closes the options with Escape and runs nothing", async () => {
    const { user } = renderScreen();
    await toSyncDialog(user);
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Sync docs-search with its upstream" }),
      ).toBeNull(),
    );
    expect(callsOf("servers_fork_sync")).toEqual([]);
  });
});

describe("servers.set-mode: the mode of a server", () => {
  it("e2e: explains there are no per-server modes, then sends the mode and shows the answer", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    expect(tools.getByText(/Toolport\+ has no per-server modes/)).toBeInTheDocument();
    await user.click(tools.getByRole("button", { name: "Change mode…" }));
    const form = await screen.findByRole("dialog", { name: "Mode of docs-search" });
    await user.selectOptions(within(form).getByLabelText("Mode"), "router");
    await user.click(within(form).getByRole("button", { name: "Review" }));

    const confirm = await screen.findByRole("dialog", {
      name: "Set the mode of docs-search?",
    });
    expect(within(confirm).getByText(/Nothing is stored/)).toBeInTheDocument();
    expect(callsOf("servers_set_mode")).toEqual([]);
    await user.click(within(confirm).getByRole("button", { name: "Set mode" }));
    const done = await screen.findByRole("dialog", {
      name: "Set the mode of docs-search",
    });
    expect(await within(done).findByText("docs-search is unchanged")).toBeInTheDocument();
    expect(within(done).getByText(/no per-server proxy modes/)).toBeInTheDocument();
    expect(callsOf("servers_set_mode")).toEqual([
      {
        tool: "servers_set_mode",
        args: { name: "docs-search", mode: "router", confirm: true },
      },
    ]);
  });

  it("sends nothing when the dialog is closed with Escape, and gives focus back", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Change mode…" }));
    await screen.findByRole("dialog", { name: "Mode of docs-search" });
    await user.keyboard("{Escape}");
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Mode of docs-search" })).toBeNull(),
    );
    expect(callsOf("servers_set_mode")).toEqual([]);
    expect(tools.getByRole("button", { name: "Change mode…" })).toHaveFocus();
  });
});

describe("servers.add-profile-tag: adding a server to a profile by name", () => {
  async function toTagDialog(user: User) {
    const { detail, tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Add to a profile…" }));
    const form = await screen.findByRole("dialog", {
      name: "Add docs-search to a profile",
    });
    return { detail, form };
  }

  it("e2e: adds to an existing profile after a plan, and the switch shows it", async () => {
    const { user } = renderScreen();
    const { form } = await toTagDialog(user);
    expect(within(form).getByText(/Not yet in: Work/)).toBeInTheDocument();
    await user.type(within(form).getByLabelText("Profile"), "work");
    expect(within(form).queryByText(/Confirming creates it/)).toBeNull();
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Add docs-search to Work?",
    });
    expect(
      within(confirm).getByText("Add docs-search to the profile Work"),
    ).toBeInTheDocument();
    expect(callsOf("servers_add_profile_tag")).toEqual([]);

    await user.click(within(confirm).getByRole("button", { name: "Add to profile" }));
    const done = await screen.findByRole("dialog", { name: "Add docs-search to Work" });
    expect(
      await within(done).findByText("docs-search is in 3 profiles"),
    ).toBeInTheDocument();
    expect(callsOf("servers_add_profile_tag")).toEqual([
      {
        tool: "servers_add_profile_tag",
        args: { name: "docs-search", profile_tag: "work" },
      },
    ]);
    await user.click(closeOf(done));
    await waitFor(() => {
      const fresh = screen.getByRole("region", { name: "docs-search details" });
      expect(
        within(fresh).getByRole("switch", { name: "docs-search in profile Work" }),
      ).toBeChecked();
    });
  });

  it("e2e: a new name creates the profile, and the plan says so first", async () => {
    const { user } = renderScreen();
    const { form } = await toTagDialog(user);
    await user.type(within(form).getByLabelText("Profile"), "Lab");
    expect(within(form).getByText(/no profile called Lab/)).toBeInTheDocument();
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Add docs-search to Lab?",
    });
    expect(within(confirm).getByText("Create the profile Lab")).toBeInTheDocument();
    expect(within(confirm).getByText(/confirming creates one/)).toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "Create and add" }));
    await screen.findByRole("dialog", { name: "Add docs-search to Lab" });
    await waitFor(() => expect(callsOf("servers_add_profile_tag")).toHaveLength(1));
    expect(world.profileLs().profiles.map((profile) => profile.name)).toContain("Lab");
  });

  it("does not offer a profile the server is already in", async () => {
    const { user } = renderScreen();
    const { form } = await toTagDialog(user);
    await user.type(within(form).getByLabelText("Profile"), "default");
    expect(within(form).getByRole("alert")).toHaveTextContent(
      "docs-search is already in Default.",
    );
    expect(within(form).getByRole("button", { name: "Review" })).toBeDisabled();
    await user.keyboard("{Escape}");
    expect(callsOf("servers_add_profile_tag")).toEqual([]);
  });

  it("shows the refusal of the tool when the profile cannot be changed", async () => {
    bridge.set("mcp call servers_add_profile_tag --args-stdin", () =>
      ctlFailure("conflict", "Profile 'work' is ambiguous; use its id"),
    );
    const { user } = renderScreen();
    const { form } = await toTagDialog(user);
    await user.type(within(form).getByLabelText("Profile"), "work");
    await user.click(within(form).getByRole("button", { name: "Review" }));
    const confirm = await screen.findByRole("dialog", {
      name: "Add docs-search to Work?",
    });
    await user.click(within(confirm).getByRole("button", { name: "Add to profile" }));
    const done = await screen.findByRole("dialog", { name: "Add docs-search to Work" });
    expect(await within(done).findByText(/ambiguous; use its id/)).toBeInTheDocument();
  });
});

describe("servers.profile-edit: taking a server out of a profile (servers_remove_profile_tag)", () => {
  it("is the same switch: it runs profile edit, and the tool is not run a second way", async () => {
    bridge.set("profile edit research --remove-server srv-docs --dry-run", {
      changed: true,
      dryRun: true,
      id: "research",
      name: "Research",
      notInProfile: [],
      oldName: "Research",
      renamed: false,
      servers: {
        added: [],
        after: ["srv-wiki", "srv-notes"],
        before: ["srv-docs", "srv-wiki", "srv-notes"],
        removed: ["srv-docs"],
      },
    });
    const { user } = renderScreen();
    const { detail } = await openServer(user, "docs-search");
    await user.click(
      within(detail).getByRole("switch", { name: "docs-search in profile Research" }),
    );
    const review = await screen.findByRole("dialog", {
      name: "Remove docs-search from Research?",
    });
    expect(review).toBeInTheDocument();
    expect(callsOf("servers_remove_profile_tag")).toEqual([]);
    expect(
      bridge
        .ran()
        .some((line) => line.startsWith("profile edit research --remove-server")),
    ).toBe(true);
  });
});

describe("the tools never put an argument in argv", () => {
  it("runs every tool as mcp call <tool> --args-stdin", async () => {
    const { user } = renderScreen();
    const { tools } = await openServer(user, "docs-search");
    await user.click(tools.getByRole("button", { name: "Where did this come from?" }));
    await tools.findByText("tracks origin/main");
    await user.click(tools.getByRole("button", { name: "Check for updates" }));
    await tools.findByText("Update available");
    const lines = bridge.calls
      .filter((call) => call.argv[0] === "mcp")
      .map((call) => call.argv.join(" "));
    expect(lines.length).toBeGreaterThanOrEqual(3);
    for (const line of lines) expect(line).toMatch(/^mcp call servers_\w+ --args-stdin$/);
  });
});
