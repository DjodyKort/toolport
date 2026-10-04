import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { open, write } from "./harness";
import { SyncTab } from "./SyncTab";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";

const PASS = "canary-passphrase-7";
let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

function configure() {
  bridge.set("sync status", golden("sync-init.after"));
  bridge.set("sync diff", golden("sync-diff.configured"));
}

describe("Sync tab: reading", () => {
  it("says sync is not set up, what it is, and what a bundle never holds", async () => {
    await open(<SyncTab />, bridge);
    expect(await screen.findByText("Not set up")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set up sync" })).toBeInTheDocument();
    const holds = screen.getByRole("group", { name: "What a bundle holds" });
    expect(
      within(holds).getByText("keychain keys, tokens, secret values"),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("sync diff");
  });

  it("shows the configuration and the differences once sync is set up", async () => {
    configure();
    await open(<SyncTab />, bridge);
    expect(await screen.findByText("m-test")).toBeInTheDocument();
    expect(screen.getByText("present")).toBeInTheDocument();
    expect(
      await screen.findByText(/This machine matches the remote bundle/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Push…" })).toBeInTheDocument();
  });

  it("lists new, changed, removed and conflicting files of the diff", async () => {
    configure();
    bridge.set("sync diff", {
      changes: {
        new: ["projects/p/new.md"],
        modified: ["settings.json"],
        removed: ["old.md"],
        conflicts: ["skills/a.md"],
        unchanged: ["x", "y"],
      },
      machineId: "m-test",
      noRemote: false,
    });
    await open(<SyncTab />, bridge);
    const list = await screen.findByRole("list", { name: "Differences" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual([
      "+projects/p/new.md(new)",
      "~settings.json(changed)",
      "−old.md(removed)",
      "!skills/a.md(conflict)",
    ]);
    expect(screen.getByText("2 unchanged")).toBeInTheDocument();
  });

  it("tells a fresh remote to push first", async () => {
    configure();
    bridge.set("sync diff", {
      changes: { new: [], modified: [], removed: [], conflicts: [], unchanged: [] },
      machineId: "m-test",
      noRemote: true,
    });
    await open(<SyncTab />, bridge);
    expect(await screen.findByText(/The remote has no bundle yet/)).toBeInTheDocument();
  });

  it("shows the failure of the settings read with Retry, and recovers", async () => {
    let fail = true;
    bridge.set("sync status", () =>
      fail ? failure("sync", "the sync state is unreadable") : golden("sync-status"),
    );
    const { user } = await open(<SyncTab />, bridge);
    expect(await screen.findByText("the sync state is unreadable")).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Not set up")).toBeInTheDocument();
  });

  it("shows a diff failure beside Retry and keeps the rest of the tab", async () => {
    configure();
    bridge.set("sync diff", failure("sync", "could not reach the remote"));
    await open(<SyncTab />, bridge);
    expect(await screen.findByText("could not reach the remote")).toBeInTheDocument();
    expect(screen.getByText("m-test")).toBeInTheDocument();
  });

  it("says when the machine is offline", async () => {
    const online = vi.spyOn(navigator, "onLine", "get").mockReturnValue(false);
    await open(<SyncTab />, bridge);
    expect(await screen.findByText(/You are offline/)).toBeInTheDocument();
    online.mockRestore();
  });

  it("shows a skeleton while the settings load", async () => {
    bridge.set("sync status", () => new Promise(() => {}));
    await open(<SyncTab />, bridge);
    expect(screen.getAllByRole("status", { name: "Loading" }).length).toBeGreaterThan(0);
  });
});

describe("Sync tab: set up", () => {
  const argv = [
    "sync",
    "init",
    "--repo",
    "git@host:me/sync.git",
    "--machine-id",
    "laptop",
    "--passphrase-stdin",
  ];

  async function fill(user: Awaited<ReturnType<typeof open>>["user"]) {
    await user.click(await screen.findByRole("button", { name: "Set up sync" }));
    const box = await screen.findByRole("dialog");
    await user.type(within(box).getByLabelText("Git repository"), "git@host:me/sync.git");
    await user.type(within(box).getByLabelText("This machine's name"), "laptop");
    return box;
  }

  it("sends the passphrase on stdin only and keeps it out of argv, the DOM and the logs", async () => {
    bridge.set(argv.join(" "), golden("sync-init.apply"));
    const { user } = await open(<SyncTab />, bridge);
    const box = await fill(user);
    expect(within(box).getByRole("button", { name: "Set up sync" })).toBeDisabled();
    await user.type(within(box).getByLabelText("Passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat passphrase"), PASS);
    expect(document.body.innerHTML).not.toContain(PASS);
    expect(
      within(box).getByText(/Set up encrypted sync with git@host:me\/sync.git/),
    ).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Set up sync" }));
    expect(
      await screen.findByText(/This machine is m-test on branch main/),
    ).toBeInTheDocument();
    expect(bridge.stdin(argv.join(" "))).toEqual([PASS]);
    expect(JSON.stringify(bridge.calls.map((call) => call.argv))).not.toContain(PASS);
    expect(bridge.ran().some((line) => line.includes("--passphrase-env"))).toBe(false);
    expect(document.body.innerHTML).not.toContain(PASS);
    expect(screen.getByRole("dialog")).toHaveTextContent("The remote was empty");
  });

  it("refuses two different passphrases, clears both fields and runs nothing", async () => {
    const { user } = await open(<SyncTab />, bridge);
    const box = await fill(user);
    await user.type(within(box).getByLabelText("Passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat passphrase"), `${PASS}-typo`);
    await user.click(within(box).getByRole("button", { name: "Set up sync" }));
    expect(await within(box).findByText(/The two entries differ/)).toBeInTheDocument();
    expect(bridge.count(argv.join(" "))).toBe(0);
    expect(within(box).getByLabelText("Passphrase")).toHaveValue("");
    expect(within(box).getByRole("button", { name: "Set up sync" })).toBeDisabled();
  });

  it("shows a failed init with its code and keeps the dialog open", async () => {
    bridge.set(argv.join(" "), failure("sync", "git clone failed: repository not found"));
    const { user } = await open(<SyncTab />, bridge);
    const box = await fill(user);
    await user.type(within(box).getByLabelText("Passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat passphrase"), PASS);
    await user.click(within(box).getByRole("button", { name: "Set up sync" }));
    expect(await screen.findByText(/repository not found/)).toBeInTheDocument();
    expect(document.body.innerHTML).not.toContain(PASS);
  });
});

describe("Sync tab: push, pull, reset, rotate", () => {
  it("pushes with a preview and a typed confirmation, because the registry says destructive", async () => {
    configure();
    bridge.set("sync push --dry-run", golden("sync-push.preview"));
    bridge.set("sync push", golden("sync-push.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await write(user, "Push…", "Push", {
      plan: "Push 1 file from m-test",
      typed: "sync push",
      done: "Pushed 1 file from m-test",
    });
    expect(bridge.ran().filter((line) => line.startsWith("sync push"))).toEqual([
      "sync push --dry-run",
      "sync push",
    ]);
  });

  it("adds --include-projects to both the preview and the apply", async () => {
    configure();
    bridge.set("sync push --include-projects --dry-run", golden("sync-push.preview"));
    bridge.set("sync push --include-projects", golden("sync-push.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await user.click(
      screen.getByRole("checkbox", { name: /Include the registered project files/ }),
    );
    await write(user, "Push…", "Push", { typed: "sync push", done: /Pushed 1 file/ });
    expect(bridge.count("sync push --include-projects --dry-run")).toBe(1);
  });

  it("does not apply when the typed phrase is wrong", async () => {
    configure();
    bridge.set("sync push --dry-run", golden("sync-push.preview"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await user.click(screen.getByRole("button", { name: "Push…" }));
    const box = await screen.findByRole("dialog");
    await user.type(await within(box).findByRole("textbox"), "push");
    expect(within(box).getByRole("button", { name: "Push" })).toBeDisabled();
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    expect(bridge.ran()).not.toContain("sync push");
  });

  it("previews a pull as a list of changes and applies it without a typed phrase", async () => {
    configure();
    const preview = golden("sync-pull.preview");
    bridge.set("sync pull --dry-run", {
      ...preview,
      changes: {
        ...preview.changes,
        new: ["projects/proj1/b.txt"],
        modified: ["settings.json"],
        conflicts: ["skills/a.md"],
      },
    });
    bridge.set("sync pull", golden("sync-pull.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await user.click(screen.getByRole("button", { name: "Pull…" }));
    const box = await screen.findByRole("dialog");
    expect(await within(box).findByText("Pull 2 files from m-test")).toBeInTheDocument();
    expect(within(box).getByText("Conflict: skills/a.md")).toBeInTheDocument();
    expect(within(box).queryByRole("textbox")).toBeNull();
    await user.click(within(box).getByRole("button", { name: "Pull" }));
    expect(await screen.findByText("Pulled 1 file from m-test")).toBeInTheDocument();
  });

  it("passes --force, --no-resolve and --run-setup to a pull when they are chosen", async () => {
    configure();
    const argv = "sync pull --force --no-resolve --run-setup";
    bridge.set(`${argv} --dry-run`, golden("sync-pull.preview"));
    bridge.set(argv, golden("sync-pull.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    for (const name of [
      /Overwrite local changes/,
      /Keep both copies/,
      /Run the setup step/,
    ]) {
      await user.click(screen.getByRole("checkbox", { name }));
    }
    await write(user, "Pull…", "Pull", { done: /Pulled 1 file/ });
    expect(bridge.ran()).toContain(argv);
  });

  it("resets sync with a typed confirmation and no preview, and shows the command line", async () => {
    configure();
    bridge.set("sync reset", golden("sync-reset.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await user.click(screen.getByRole("button", { name: "Reset…" }));
    const box = await screen.findByRole("dialog");
    expect(
      within(box).getByText("Remove the remote sync data and the local sync state"),
    ).toBeInTheDocument();
    expect(within(box).getByLabelText("Command line")).toHaveTextContent(
      "toolportctl sync reset",
    );
    expect(within(box).getByText(/no preview/)).toBeInTheDocument();
    await user.type(within(box).getByRole("textbox"), "sync reset");
    await user.click(within(box).getByRole("button", { name: "Reset sync" }));
    expect(await screen.findByText("Sync data removed")).toBeInTheDocument();
    expect(bridge.ran()).toContain("sync reset");
  });

  it("rotates the passphrase on stdin with a typed confirmation and a repeat", async () => {
    configure();
    const argv = "sync rotate-passphrase --passphrase-stdin";
    bridge.set(argv, golden("sync-rotate-passphrase.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    await user.click(screen.getByRole("button", { name: "Rotate passphrase" }));
    const box = await screen.findByRole("dialog");
    await user.type(within(box).getByLabelText("New passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat new passphrase"), PASS);
    expect(within(box).getByRole("button", { name: "Rotate" })).toBeDisabled();
    await user.type(
      within(box).getByRole("textbox", { name: /Type sync rotate-passphrase/ }),
      "sync rotate-passphrase",
    );
    await user.click(within(box).getByRole("button", { name: "Rotate" }));
    expect(await screen.findByText(/1 blob\(s\) re-encrypted/)).toBeInTheDocument();
    expect(bridge.stdin(argv)).toEqual([PASS]);
    expect(JSON.stringify(bridge.calls.map((call) => call.argv))).not.toContain(PASS);
    expect(document.body.innerHTML).not.toContain(PASS);
  });
});

describe("Sync tab: projects, git sync, migrate", () => {
  it("lists the projects and removes one after a confirmation", async () => {
    bridge.set("sync status", golden("sync-add-project.after"));
    bridge.set("sync diff", golden("sync-diff.configured"));
    bridge.set("sync remove-project proj1", golden("sync-remove-project.apply"));
    const { user } = await open(<SyncTab />, bridge);
    const list = await screen.findByRole("list", { name: "Projects" });
    expect(within(list).getByText("proj1")).toBeInTheDocument();
    expect(within(list).getByText(/a\.txt/)).toBeInTheDocument();
    await write(user, "Remove proj1 from the sync set", "Remove", {
      plan: "Remove the project proj1 from the sync set",
      done: "Removed proj1 from the sync set",
    });
    expect(bridge.ran()).toContain("sync remove-project proj1");
  });

  it("adds a project with its name and files", async () => {
    configure();
    const argv = "sync add-project /work/app --name app --files CLAUDE.md,.env.example";
    bridge.set(argv, golden("sync-add-project.apply"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("m-test");
    expect(screen.getByRole("button", { name: "Add project…" })).toBeDisabled();
    await user.type(screen.getByLabelText("Project folder"), "/work/app");
    await user.type(screen.getByLabelText("Name in the sync set"), "app");
    await user.type(screen.getByLabelText("Files"), "CLAUDE.md, .env.example");
    await write(user, "Add project…", "Add project", {
      done: "Added app to the sync set",
    });
    expect(bridge.ran()).toContain(argv);
  });

  it("sets up git sync with the repository, branch and auto flag", async () => {
    const argv = "sync git-sync --repo git@host:me/data.git --branch main --auto";
    bridge.set(argv, golden("sync-git-sync.configure"));
    const { user } = await open(<SyncTab />, bridge);
    await screen.findByText("Git sync is not set up.");
    await user.type(
      screen.getByLabelText("Git repository", { selector: "input" }),
      "git@host:me/data.git",
    );
    await user.type(screen.getByLabelText("Git branch"), "main");
    await user.click(screen.getByRole("checkbox", { name: /Sync automatically/ }));
    await write(user, "Set up git sync…", "Set up git sync", {
      done: "Git sync is set up with git@host:me/data.git",
    });
    expect(bridge.ran()).toContain(argv);
  });

  it("shows a configured git sync and removes it with --clear", async () => {
    bridge.set("sync git-sync --status", {
      ...golden("sync-git-sync.configure"),
      autoSync: true,
    });
    bridge.set("sync git-sync --clear", golden("sync-git-sync.clear"));
    const { user } = await open(<SyncTab />, bridge);
    expect(
      await screen.findByText("/world/data/skills_repo".replace("/world", "<WORLD>")),
    ).toBeInTheDocument();
    await write(user, "Remove setup…", "Remove setup", {
      done: "Git sync setup removed",
    });
    expect(bridge.ran()).toContain("sync git-sync --clear");
  });

  it("imports an mcpm bundle with its passphrase on stdin", async () => {
    const argv = "sync migrate /old/bundle --passphrase-stdin";
    bridge.set(argv, { migrated: 3 });
    const { user } = await open(<SyncTab />, bridge);
    await user.click(await screen.findByRole("button", { name: "Import bundle…" }));
    const box = await screen.findByRole("dialog");
    expect(within(box).getByRole("button", { name: "Import bundle" })).toBeDisabled();
    await user.type(within(box).getByLabelText("Bundle folder"), "/old/bundle");
    await user.type(within(box).getByLabelText("Bundle passphrase"), PASS);
    await user.click(within(box).getByRole("button", { name: "Import bundle" }));
    expect(await screen.findByText("Bundle imported")).toBeInTheDocument();
    expect(bridge.stdin(argv)).toEqual([PASS]);
    expect(document.body.innerHTML).not.toContain(PASS);
  });
});
