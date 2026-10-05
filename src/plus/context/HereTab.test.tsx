import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { finish, mountContext, review, section, visibleText } from "./e2e";
import {
  CANARY,
  FOLDER,
  HOME_DIR,
  OTHER,
  loadsFolder,
  seedHere,
  seedProfiles,
} from "./tabsKit";
import { createBridge, failure, goldenData, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge();
  seedHere(bridge);
  wire({ invoke, listen }, bridge);
});
afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(bridge.ran().some((line) => /--home|--reveal|secret/.test(line))).toBe(false);
  expect(bridge.stdins()).toEqual([]);
});

async function open() {
  const user = mountContext("here");
  await screen.findByRole("group", { name: "Skill list budget" });
  return user;
}

async function showFolder(user: ReturnType<typeof mountContext>, folder = FOLDER) {
  await user.type(screen.getByLabelText("Folder"), folder);
  await user.click(screen.getByRole("button", { name: "Show" }));
  await waitFor(() =>
    expect(screen.getByText(folder, { selector: "code" })).toBeVisible(),
  );
  await screen.findByRole("group", { name: "Skill list budget" });
}

const rowOf = (scope: { getAllByRole: (role: string) => HTMLElement[] }, name: string) =>
  scope
    .getAllByRole("listitem")
    .find((li) => li.querySelector("b")?.textContent === name)!;

describe("This folder: reading", () => {
  it("is the default tab and reads the home folder with one measured-aware loads call", async () => {
    mountContext("here");
    await screen.findByRole("tab", { name: "This folder", selected: true });
    await screen.findByRole("group", { name: "Skill list budget" });
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "context loads --measured",
        `context compose --cwd ${HOME_DIR}`,
        `context bundle status --cwd ${HOME_DIR}`,
        "context bundle ls",
      ]),
    );
    expect(bridge.ran().filter((line) => /--dry-run|--yes/.test(line))).toEqual([]);
  });

  it("shows the stack in groups, with the origin of every row and the on-demand rows apart", async () => {
    const user = await open();
    await showFolder(user);
    const stack = within(section("Stack"));
    const headings = stack
      .getAllByRole("heading", { level: 4 })
      .map((h) => h.textContent);
    expect(headings).toEqual([
      "Instructions",
      "Skills, commands and agents",
      "Plugins",
      "Tools (MCP)",
      "Memory",
      "Settings",
      "Loads on demand",
      "Not loaded here",
    ]);
    const instructions = within(
      screen.getByRole("region", { name: "Instructions" }),
    ).getAllByRole("listitem");
    const org = instructions.find((li) => li.textContent?.includes("corp-tools"))!;
    expect(within(org).getByText("10,070")).toBeVisible();
    expect(within(org).getByText("estimate")).toBeVisible();
    const demand = within(screen.getByRole("region", { name: "Loads on demand" }));
    expect(demand.getByText("client-acme-erp")).toBeVisible();
    expect(demand.getAllByText("on demand").length).toBeGreaterThan(0);
    const never = within(screen.getByRole("region", { name: "Not loaded here" }));
    expect(never.getAllByText("not loaded").length).toBeGreaterThan(0);
    expect(never.getByText("excluded-notes/CLAUDE.md")).toBeVisible();
  });

  it("labels every token number as an estimate until it was measured, and shows no saving", async () => {
    const user = await open();
    await showFolder(user);
    const summary = within(screen.getByRole("group", { name: "What loads, in numbers" }));
    expect(summary.getByText("not measured yet")).toBeVisible();
    expect(summary.getByText("Your files, estimated")).toBeVisible();
    for (const row of screen.getAllByLabelText(
      /tokens, (estimate|measured|projected)$/,
    )) {
      expect(row.getAttribute("aria-label")).toMatch(/, estimate$/);
    }
    expect(screen.queryByRole("list", { name: "Measured savings" })).toBeNull();
    expect(visibleText()).not.toMatch(/\bsaves?\b|saving of/i);
  });

  it("shows the skill-list budget as a meter and says which skills lose their description", async () => {
    const user = await open();
    await showFolder(user);
    const meter = screen.getByRole("meter", { name: "Skill list budget" });
    expect(meter).toHaveAttribute("aria-valuenow", "1998");
    expect(meter).toHaveAttribute("aria-valuemax", "2000");
    const group = within(screen.getByRole("group", { name: "Skill list budget" }));
    expect(group.getByText(/17 skills lose their description/)).toBeVisible();
    expect(group.getByText(/a guess until the folder is measured/)).toBeVisible();
  });

  it("collapses a long group to four rows and shows all on request", async () => {
    const user = await open();
    await showFolder(user);
    const skills = within(
      screen.getByRole("region", { name: "Skills, commands and agents" }),
    );
    const before = skills.getAllByRole("listitem").length;
    expect(before).toBe(4);
    await user.click(skills.getByRole("button", { name: /Show all/ }));
    expect(skills.getAllByRole("listitem").length).toBeGreaterThan(before);
    await user.click(skills.getByRole("button", { name: "Show fewer" }));
    expect(skills.getAllByRole("listitem")).toHaveLength(before);
  });

  it("asks for the folder in a path field with recents, remembers it and never sends --home", async () => {
    const user = await open();
    await showFolder(user, FOLDER);
    expect(bridge.ran()).toContain(`context loads --cwd ${FOLDER} --measured`);
    expect(
      JSON.parse(window.localStorage.getItem("toolport.context.recent-folders")!),
    ).toEqual([FOLDER]);
    const options = [...document.querySelectorAll("datalist option")].map((o) =>
      o.getAttribute("value"),
    );
    expect(options).toContain(FOLDER);
  });

  it("keeps its folder when another tab is opened and back", async () => {
    seedProfiles(bridge);
    const user = await open();
    await showFolder(user, FOLDER);
    await user.click(screen.getByRole("tab", { name: "Profiles" }));
    await screen.findByRole("region", { name: "Profile acme-dev" });
    await user.click(screen.getByRole("tab", { name: "This folder" }));
    expect(await screen.findByLabelText("Folder")).toHaveValue(FOLDER);
    await screen.findByRole("group", { name: "Skill list budget" });
    expect(screen.getByText(FOLDER, { selector: "code" })).toBeVisible();
  });

  it("starts from the newest recent folder", async () => {
    window.localStorage.setItem(
      "toolport.context.recent-folders",
      JSON.stringify([OTHER, FOLDER]),
    );
    mountContext("here");
    await screen.findByRole("group", { name: "Skill list budget" });
    expect(screen.getByLabelText("Folder")).toHaveValue(OTHER);
    expect(bridge.ran()).toContain(`context loads --cwd ${OTHER} --measured`);
    expect(bridge.ran()).not.toContain("context loads --measured");
  });

  it("keeps working when browser storage is blocked", async () => {
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {
      throw new Error("blocked");
    };
    try {
      const user = await open();
      await showFolder(user);
      expect(screen.getByRole("meter", { name: "Skill list budget" })).toBeVisible();
    } finally {
      Storage.prototype.setItem = original;
    }
  });
});

describe("This folder: states", () => {
  it("shows a skeleton while the stack is read", async () => {
    bridge.set("context loads --measured", () => new Promise(() => {}));
    mountContext("here");
    const stack = await screen.findByRole("region", { name: "Stack" });
    expect(within(stack).getByRole("status", { name: "Loading" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
  });

  it("says nothing loads when the folder has no rows", async () => {
    bridge.set("context loads --measured", { ...loadsFolder(HOME_DIR), items: [] });
    mountContext("here");
    expect(await screen.findByText("Nothing loads in this folder.")).toBeVisible();
  });

  it("shows the CLI's words with Retry for a failed read and recovers", async () => {
    bridge.set("context loads --measured", failure("io", "cannot read the home"));
    const user = mountContext("here");
    const stack = within(await screen.findByRole("region", { name: "Stack" }));
    expect(await stack.findByRole("alert")).toHaveTextContent("cannot read the home");
    bridge.set("context loads --measured", loadsFolder(HOME_DIR));
    await user.click(stack.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("group", { name: "Skill list budget" })).toBeVisible();
  });

  it("marks a partial read, and shows a failed profile read without breaking the stack", async () => {
    bridge.set("context loads --measured", { ...loadsFolder(HOME_DIR), partial: true });
    bridge.set(`context bundle status --cwd ${HOME_DIR}`, failure("io", "cannot read"));
    mountContext("here");
    expect(await screen.findByText(/Some sources could not be read/)).toBeVisible();
    expect(await screen.findByText("could not be read")).toBeVisible();
    expect(screen.getByRole("meter", { name: "Skill list budget" })).toBeVisible();
  });

  it("shows every read as failed while toolportctl is down, and keeps Measure off", async () => {
    invoke.mockReset().mockImplementation(async (command: string) => {
      if (command === "plus_ctl") throw new Error("toolportctl was not found");
      throw new Error(`unexpected invoke ${command}`);
    });
    mountContext("here");
    await waitFor(() =>
      expect(screen.getAllByText(/toolportctl was not found/).length).toBeGreaterThan(0),
    );
    expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Measure for real…" })).toBeDisabled();
  });
});

describe("This folder: measuring for real", () => {
  const measured = () => goldenData("context-measure.measured");

  async function measureConfirm(user: ReturnType<typeof mountContext>) {
    await user.click(screen.getByRole("button", { name: "Measure for real…" }));
    return review(/Measure what Claude really loads here\?/);
  }

  it("asks first, because it spends requests: nothing runs before the confirm", async () => {
    bridge.set(`context measure --cwd ${FOLDER} --yes`, measured());
    const user = await open();
    await showFolder(user);
    const box = await measureConfirm(user);
    expect(box.getByText(/spends model tokens/)).toBeVisible();
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      `toolportctl context measure --cwd ${FOLDER} --yes`,
    );
    expect(box.queryByText(/has no preview/)).toBeNull();
    expect(bridge.ran().filter((line) => line.startsWith("context measure"))).toEqual([]);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.ran().filter((line) => line.startsWith("context measure"))).toEqual([]);
  });

  it("measures after the confirm, shows the real number with its basis and reads the stack again", async () => {
    let done = false;
    bridge.set(`context measure --cwd ${FOLDER} --yes`, () => {
      done = true;
      return measured();
    });
    bridge.set(`context loads --cwd ${FOLDER} --measured`, () => {
      const data = loadsFolder();
      if (!done) return data;
      const run = goldenData("context-measure.measured").runs[0];
      return {
        ...data,
        measured: run,
        measured_info: {
          claudeCodeVersion: "2.1.289",
          model: "claude-haiku-4-5-20251001",
          measuredAt: "2026-10-05T09:00:00Z",
          stale: false,
        },
      };
    });
    const user = await open();
    await showFolder(user);
    expect(bridge.count(`context loads --cwd ${FOLDER} --measured`)).toBe(1);
    await measureConfirm(user);
    await finish(user, /Measure what Claude really loads here\?/, "Measure", {
      done: "68,445",
    });
    expect(bridge.ran()).toContain(`context measure --cwd ${FOLDER} --yes`);
    await waitFor(() =>
      expect(bridge.count(`context loads --cwd ${FOLDER} --measured`)).toBe(2),
    );
    const summary = within(screen.getByRole("group", { name: "What loads, in numbers" }));
    expect(await summary.findByText("68,445 tokens")).toBeVisible();
    expect(summary.getByText(/first request · Claude Code 2\.1\.289/)).toBeVisible();
  });

  it("shows the measured saving only from a measurement, per plugin, with --without", async () => {
    bridge.set(
      `context measure --cwd ${FOLDER} --without plugin:kit@market --yes`,
      measured(),
    );
    const user = await open();
    await showFolder(user);
    const plugins = within(screen.getByRole("region", { name: "Plugins" }));
    const kit = rowOf(plugins, "kit@market");
    const idle = within(screen.getByRole("region", { name: "Not loaded here" }));
    expect(within(rowOf(idle, "idle@market")).queryByRole("button")).toBeNull();
    await user.click(within(kit).getByRole("button", { name: "Measure without it…" }));
    const box = await review(/Measure without kit@market\?/);
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      `--without plugin:kit@market --yes`,
    );
    expect(bridge.ran().filter((line) => line.startsWith("context measure"))).toEqual([]);
    await user.click(box.getByRole("button", { name: "Measure" }));
    const result = within(await screen.findByRole("region", { name: "Measured" }));
    const savings = within(await result.findByRole("list", { name: "Measured savings" }));
    expect(
      savings.getByText(/without plugin:kit@market: −8,651 tokens \(-12\.6%\), measured/),
    ).toBeVisible();
    expect(result.getByText(/1 skill Claude Code does not list: handoff/)).toBeVisible();
  });

  it("opens the turn-off command of a plugin from its row, runs nothing, and returns the focus", async () => {
    const user = await open();
    await showFolder(user);
    const plugins = within(screen.getByRole("region", { name: "Plugins" }));
    const kit = rowOf(plugins, "kit@market");
    const opener = within(kit).getByRole("button", { name: "Off here…" });
    await user.click(opener);
    const box = within(
      await screen.findByRole("dialog", { name: /Turn kit@market off in a folder/ }),
    );
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      "claude plugin disable kit@market --scope local",
    );
    expect(box.getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    expect(bridge.ran().some((line) => /disable|settings\.local/.test(line))).toBe(false);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(opener).toHaveFocus());
  });

  it("shows the failure of a measurement in the CLI's words and keeps the screen", async () => {
    bridge.set(
      `context measure --cwd ${FOLDER} --yes`,
      failure("claude_failed", "claude -p exited with status 1"),
    );
    const user = await open();
    await showFolder(user);
    await measureConfirm(user);
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Measure" }),
    );
    expect(await screen.findByText(/claude -p exited with status 1/)).toBeVisible();
    const done = within(screen.getByRole("dialog"));
    await user.click(done.getAllByRole("button", { name: "Close" }).at(-1)!);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("meter", { name: "Skill list budget" })).toBeVisible();
  });
});

describe("This folder: the applied profile and the composed text", () => {
  it("says none, and sends the person to the Profiles tab with the folder remembered", async () => {
    seedProfiles(bridge);
    const user = await open();
    await showFolder(user);
    expect(screen.getByText("Profile applied here:").parentElement).toHaveTextContent(
      "none",
    );
    await user.click(screen.getByRole("button", { name: "Apply a profile…" }));
    await screen.findByRole("tab", { name: "Profiles", selected: true });
    expect(
      JSON.parse(window.localStorage.getItem("toolport.context.recent-folders")!)[0],
    ).toBe(FOLDER);
  });

  it("names the profile applied here and flags a drift", async () => {
    bridge.set(
      `context bundle status --cwd ${HOME_DIR}`,
      goldenData("context-bundle-status.drift"),
    );
    mountContext("here");
    const line = (await screen.findByText("Profile applied here:")).parentElement!;
    expect(await within(line).findByText("acme-dev")).toBeVisible();
    expect(within(line).getByText("changed since the apply")).toBeVisible();
  });

  it("shows the composed text collapsed per part, with the layers that fed it", async () => {
    const user = await open();
    await showFolder(user);
    const composed = within(section("Composed instructions"));
    expect(
      await composed.findByText(
        (_, node) =>
          node?.tagName === "P" && node.textContent === "143 tokens in total, estimate.",
      ),
    ).toBeVisible();
    const parts = within(
      composed.getByRole("list", { name: "Composed parts" }),
    ).getAllByRole("listitem");
    expect(parts).toHaveLength(3);
    expect(
      within(parts[2]).getByText(/layers: client-acme-erp, client-erp-knowledge/),
    ).toBeVisible();
    for (const details of document.querySelectorAll("details")) {
      expect((details as HTMLDetailsElement).open).toBe(false);
    }
    await user.click(within(parts[0]).getByText("Show the text"));
    expect(within(parts[0]).getByText(/Be brief\./)).toBeVisible();
  });

  it("renders no file content beyond what compose returns, and never the profile yaml", async () => {
    bridge.set(`context compose --cwd ${HOME_DIR}`, {
      ...goldenData("context-compose.layers"),
      cwd: HOME_DIR,
      skipped: [{ path: null, reason: "an import outside the folder" }],
    });
    bridge.set("context bundle show acme-dev", {
      ...goldenData("context-bundle-show.bundle"),
      yaml: `token: ${CANARY}\n`,
    });
    mountContext("here");
    await screen.findByRole("list", { name: "Composed parts" });
    expect(visibleText()).not.toContain(CANARY);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    expect(
      window.localStorage.getItem("toolport.context.recent-folders") ?? "",
    ).not.toContain(CANARY);
  });
});

describe("This folder: keyboard", () => {
  it("submits the folder with Enter and moves between the tabs with the arrow keys", async () => {
    seedProfiles(bridge);
    const user = await open();
    await user.type(screen.getByLabelText("Folder"), `${OTHER}{Enter}`);
    await waitFor(() =>
      expect(bridge.ran()).toContain(`context loads --cwd ${OTHER} --measured`),
    );
    const tab = screen.getByRole("tab", { name: "This folder" });
    tab.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Profiles" })).toHaveFocus();
  });

  it("opens the measure confirmation from the keyboard and cancels it with Escape", async () => {
    const user = await open();
    await showFolder(user);
    const button = screen.getByRole("button", { name: "Measure for real…" });
    button.focus();
    await user.keyboard("{Enter}");
    const box = await review(/Measure what Claude really loads here\?/);
    expect(box.getByRole("button", { name: "Measure" })).toBeVisible();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.ran().filter((line) => line.startsWith("context measure"))).toEqual([]);
  });
});
