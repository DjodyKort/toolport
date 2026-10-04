import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { PlusViews } from "../PlusViews";
import { render } from "@testing-library/react";
import { button, confirm, group, openSystem, pageText } from "./e2e";
import { createBridge, failure, wire, type Bridge } from "./testkit";
import type { SystemState } from "./world";

/** The Sync tab walked the way a person uses it, against a world that changes: an init
 * configures it, a push moves the bundle and the last sync, a pull brings another machine's
 * files over. Each test is named by the parity action it proves (`src/plus/gui-parity.json`)
 * and asserts that the next read changed. */
let bridge: Bridge;
const start = (world: boolean | Partial<SystemState> = true) => {
  bridge = createBridge({ world });
  wire({ invoke, listen }, bridge);
};
beforeEach(() => start());

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge
    .ran()
    .filter(
      (line) =>
        !/^(commands|sync|update|council|mcp|import|secret set council)( |$)/.test(line),
    );
  expect(stray, "the screen only runs its own command groups").toEqual([]);
  expect(bridge.ran().some((line) => /CANARY/.test(line))).toBe(false);
});

const PASS = "CANARY-phrase-7d1e";
const REPO = "git@git.example.com:me/toolport-sync.git";

async function setUp(user: ReturnType<typeof userEvent.setup>, extra = "") {
  await user.click(await screen.findByRole("button", { name: "Set up sync" }));
  const box = await screen.findByRole("dialog", { name: "Set up sync?" });
  await user.type(within(box).getByLabelText("Git repository"), REPO);
  await user.type(within(box).getByLabelText("This machine's name"), "work-laptop");
  await user.type(within(box).getByLabelText("Passphrase"), PASS);
  await user.type(within(box).getByLabelText("Repeat passphrase"), PASS + extra);
  return box;
}

describe("Sync tab, end to end", () => {
  it("system.sync-status: a fresh machine is Not set up, and says what a bundle holds", async () => {
    await openSystem(bridge);
    expect(await screen.findByText("Not set up")).toBeVisible();
    expect(group("What a bundle holds").getByText(/keychain keys, tokens/)).toBeVisible();
    expect(bridge.count("sync status")).toBe(1);
    expect(bridge.count("sync diff")).toBe(0);
  });

  it("system.sync-init: the passphrase goes to stdin only, a mismatch clears both, and the next read is configured", async () => {
    const user = await openSystem(bridge);
    let box = await setUp(user, "x");
    await user.click(within(box).getByRole("button", { name: "Set up sync" }));
    expect(await within(box).findByText(/The two entries differ/)).toBeVisible();
    expect(within(box).getByLabelText("Passphrase")).toHaveValue("");
    expect(
      bridge.count(
        `sync init --repo ${REPO} --machine-id work-laptop --passphrase-stdin`,
      ),
    ).toBe(0);
    await user.type(within(box).getByLabelText("Passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat passphrase"), PASS);
    expect(pageText()).not.toContain("CANARY-phrase-7d1e\u0000");
    await user.click(within(box).getByRole("button", { name: "Set up sync" }));
    expect(
      await within(box).findByText(/This machine is work-laptop on branch main/),
    ).toBeVisible();
    expect(pageText().replace(new RegExp(`value="${PASS}"`, "g"), "")).not.toContain(
      PASS,
    );
    expect(screen.queryByDisplayValue(PASS)).toBeNull();
    const argv = `sync init --repo ${REPO} --machine-id work-laptop --passphrase-stdin`;
    expect(bridge.stdin(argv)).toEqual([PASS]);
    expect(bridge.ran().join("\n")).not.toContain(PASS);
    await user.click(within(box).getByRole("button", { name: "Done" }));
    expect(await screen.findByText("work-laptop")).toBeVisible();
    expect(group("Encrypted sync").getByText(REPO)).toBeVisible();
    expect(await screen.findByText(/The remote has no bundle yet/)).toBeVisible();
    box = screen.getByRole("group", { name: "Encrypted sync" });
    expect(within(box).getByText("present")).toBeVisible();
  });

  it("system.sync-push: a preview changes nothing, Escape cancels and returns focus, the apply moves the last sync", async () => {
    start({ sync: configured() });
    const user = await openSystem(bridge);
    expect(
      await within(
        await screen.findByRole("group", { name: "Encrypted sync" }),
      ).findByText("Never"),
    ).toBeVisible();
    const push = button("Push…");
    await user.click(push);
    const box = await screen.findByRole("dialog", {
      name: "Push to the sync repository?",
    });
    expect(within(box).getByText("registry.json")).toBeVisible();
    expect(
      within(box).getByText(/Keychain keys, tokens and secret values are never/),
    ).toBeVisible();
    expect(within(box).getByRole("button", { name: "Push" })).toBeDisabled();
    expect(bridge.ran()).toContain("sync push --dry-run");
    expect(bridge.count("sync push")).toBe(0);
    expect(bridge.world!.state.sync.remote).toBeNull();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(push).toHaveFocus();
    expect(bridge.count("sync push")).toBe(0);

    await user.click(push);
    await confirm(user, "Push", {
      typed: "sync push",
      done: /^Pushed 3 files from work-laptop/,
    });
    await waitFor(() => expect(group("Encrypted sync").queryByText("Never")).toBeNull());
    expect(group("Encrypted sync").getByText(/\(push\)/)).toBeVisible();
    expect(group("Encrypted sync").getByText("3")).toBeVisible();
    expect(
      await screen.findByText(/This machine matches the remote bundle/),
    ).toBeVisible();
  });

  it("system.sync-diff, system.sync-pull: another machine's push shows in the diff, the preview keeps it, the apply clears it", async () => {
    start({ sync: configured({ pushed: true }) });
    bridge.world!.remoteEdit("registry.json", "v2");
    bridge.world!.remoteEdit("skills/new/SKILL.md", "v1");
    const user = await openSystem(bridge);
    const diff = await screen.findByRole("list", { name: "Differences" });
    expect(within(diff).getByText("skills/new/SKILL.md")).toBeVisible();
    expect(within(diff).getByText("registry.json")).toBeVisible();
    await user.click(button("Pull…"));
    const box = await screen.findByRole("dialog", {
      name: "Pull from the sync repository?",
    });
    expect(within(box).getByText("Pull 2 files from work-laptop")).toBeVisible();
    expect(bridge.ran()).toContain("sync pull --dry-run");
    expect(bridge.world!.state.sync.local["registry.json"]).toBe("v1");
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    await user.click(button("Check differences again"));
    await waitFor(() => expect(bridge.count("sync diff")).toBe(2));
    expect(screen.getByRole("list", { name: "Differences" })).toBeVisible();

    await user.click(button("Pull…"));
    await confirm(user, "Pull", { done: /^Pulled 2 files from work-laptop/ });
    expect(bridge.world!.state.sync.local["registry.json"]).toBe("v2");
    expect(
      await screen.findByText(/This machine matches the remote bundle/),
    ).toBeVisible();
    expect(screen.queryByRole("list", { name: "Differences" })).toBeNull();
  });

  it("system.sync-project-add, system.sync-project-remove: a project joins the sync set and a push with projects carries its files", async () => {
    start({ sync: configured({ pushed: true }) });
    const user = await openSystem(bridge);
    const card = await screen.findByRole("group", { name: "Projects in the sync set" });
    await user.type(within(card).getByLabelText("Project folder"), "/work/client-repo");
    await user.type(within(card).getByLabelText("Name in the sync set"), "client-repo");
    await user.click(within(card).getByRole("button", { name: "Add project…" }));
    await confirm(user, "Add project", { done: "Added client-repo to the sync set" });
    expect(await within(card).findByRole("list", { name: "Projects" })).toHaveTextContent(
      "client-repo",
    );
    await user.click(screen.getByLabelText("Include the registered project files"));
    await user.click(button("Push…"));
    const box = await screen.findByRole("dialog", {
      name: "Push to the sync repository?",
    });
    expect(await within(box).findByText("projects/client-repo/CLAUDE.md")).toBeVisible();
    expect(bridge.ran()).toContain("sync push --include-projects --dry-run");
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    await user.click(
      screen.getByRole("button", { name: "Remove client-repo from the sync set" }),
    );
    await confirm(user, "Remove", { done: "Removed client-repo from the sync set" });
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Projects" })).toBeNull(),
    );
    expect(bridge.world!.state.sync.projects).toEqual({});
  });

  it("system.sync-git: git sync is set up and removed, and the card follows", async () => {
    start({ sync: configured({ pushed: true }) });
    const user = await openSystem(bridge);
    const card = await screen.findByRole("group", {
      name: "Git sync of the data folder",
    });
    expect(await within(card).findByText("Git sync is not set up.")).toBeVisible();
    await user.type(
      within(card).getByLabelText("Git repository"),
      "git@git.example.com:me/data.git",
    );
    await user.click(within(card).getByRole("button", { name: "Set up git sync…" }));
    await confirm(user, "Set up git sync", { done: /Git sync is set up with/ });
    expect(
      await within(card).findByText("git@git.example.com:me/data.git"),
    ).toBeVisible();
    await user.click(within(card).getByRole("button", { name: "Remove setup…" }));
    await confirm(user, "Remove setup", {
      done: "Git sync setup removed",
    });
    expect(await within(card).findByText("Git sync is not set up.")).toBeVisible();
  });

  it("system.sync-rotate, system.sync-reset: rotating takes the passphrase on stdin and a typed phrase, a reset ends in Not set up", async () => {
    start({ sync: configured({ pushed: true }) });
    const user = await openSystem(bridge);
    await user.click(await screen.findByRole("button", { name: /Rotate passphrase/ }));
    const box = await screen.findByRole("dialog", { name: "Rotate the passphrase?" });
    await user.type(within(box).getByLabelText("New passphrase"), PASS);
    await user.type(within(box).getByLabelText("Repeat new passphrase"), PASS);
    expect(within(box).getByRole("button", { name: "Rotate" })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), "sync rotate-passphrase");
    await user.click(within(box).getByRole("button", { name: "Rotate" }));
    expect(await within(box).findByText(/blob\(s\) re-encrypted/)).toBeVisible();
    expect(bridge.stdin("sync rotate-passphrase --passphrase-stdin")).toEqual([PASS]);
    expect(screen.queryByDisplayValue(PASS)).toBeNull();
    await user.click(within(box).getByRole("button", { name: "Done" }));

    await user.click(button("Reset…"));
    await confirm(user, "Reset sync", {
      typed: "sync reset",
      done: "Local sync setup removed",
    });
    expect(await screen.findByText("Not set up")).toBeVisible();
    expect(bridge.world!.state.sync.configured).toBe(false);
  });

  it("system.sync-migrate: a folder without a bundle fails with its reason and the passphrase never reaches argv", async () => {
    const user = await openSystem(bridge);
    await user.click(await screen.findByRole("button", { name: /Import bundle…/ }));
    const box = await screen.findByRole("dialog", {
      name: "Import an mcpm sync bundle?",
    });
    await user.type(within(box).getByLabelText("Bundle folder"), "/old/mcpm-sync");
    await user.type(within(box).getByLabelText("Bundle passphrase"), PASS);
    await user.click(within(box).getByRole("button", { name: "Import bundle" }));
    expect(await within(box).findByText(/sync_manifest.json/)).toBeVisible();
    expect(bridge.stdin("sync migrate /old/mcpm-sync --passphrase-stdin")).toEqual([
      PASS,
    ]);
  });

  it("system.sync-status: toolportctl being down is said, and Retry recovers", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    render(<PlusViews view="system" onSelectView={() => {}} />);
    const failed = (await screen.findByText("Couldn't read the sync settings")).closest(
      '[role="alert"]',
    ) as HTMLElement;
    expect(failed).toHaveTextContent(/toolportctl could not be started/);
    down = false;
    await userEvent.setup().click(within(failed).getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("Not set up")).toBeVisible();
    bridge.set("sync diff", failure("sync", "unused"));
  });
});

function configured(options: { pushed?: boolean } = {}) {
  const base = {
    configured: true,
    repo: REPO,
    machineId: "work-laptop",
  };
  const local = {
    "registry.json": "v1",
    "profiles.json": "v1",
    "skills/review/SKILL.md": "v1",
  };
  return options.pushed
    ? {
        ...base,
        branch: "main",
        lastSyncAt: "2026-10-04T09:00:00Z",
        lastDirection: "push",
        local,
        remote: { ...local },
        synced: { ...local },
        projects: {},
      }
    : {
        ...base,
        branch: "main",
        lastSyncAt: "",
        lastDirection: "",
        local,
        remote: null,
        synced: {},
        projects: {},
      };
}
