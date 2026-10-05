import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import type {
  LibraryPullData,
  LibraryPushData,
  LibraryStatusData,
} from "../../types/library";
import { SourcesTab } from "./SourcesTab";
import {
  createSourcesBridge,
  failure,
  goldenFailure,
  libraryGolden,
  wire,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createSourcesBridge();
  wire({ invoke, listen }, bridge);
});

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(
    bridge
      .ran()
      .some((line) => /--force|--home|--data-dir|secret|stdin|token/.test(line)),
  ).toBe(false);
});

const status = (stem: string) =>
  libraryGolden<LibraryStatusData>(`library-status.${stem}`);
const library = () => within(screen.getByRole("group", { name: "Library remote" }));

async function open(wait = true) {
  const user = userEvent.setup();
  render(<SourcesTab />);
  const list = await screen.findByRole("list", { name: "Sources" });
  await user.click(
    within(list)
      .getAllByRole("button")
      .find((b) => within(b).queryByText("ai-skills", { selector: "b" }))!,
  );
  if (wait) await library().findByText("Changes here");
  return user;
}

describe("Library row: status", () => {
  it("shows the remote, how far behind it is and that nothing is changed here", async () => {
    await open();
    const box = library();
    expect(box.getByText("/fixture/lib-remote.git")).toBeVisible();
    expect(box.getByText("Branch main, follows origin/main")).toBeVisible();
    expect(box.getByText("2 commits behind, 0 commits ahead")).toBeVisible();
    expect(box.getByText("Nothing changed here")).toBeVisible();
    expect(box.getByText(/your git credentials; not tried yet/)).toBeVisible();
    expect(box.getByRole("button", { name: "Pull" })).toBeEnabled();
    expect(box.getByRole("button", { name: "Push…" })).toBeEnabled();
    expect(bridge.ran().filter((line) => line.startsWith("library"))).toEqual([
      "library status",
    ]);
  });

  it("says a commit is not pushed when the clone is ahead", async () => {
    bridge.set("library status", status("ahead"));
    await open();
    expect(library().getByText("0 commits behind, 1 commit ahead")).toBeVisible();
    expect(library().getByText("1 commit not pushed")).toBeVisible();
    expect(library().getByText(/Last fetch never/)).toBeVisible();
  });

  it("counts uncommitted files and warns that Pull will refuse", async () => {
    bridge.set("library status", status("dirty"));
    await open();
    expect(library().getByText("2 files changed")).toBeVisible();
    expect(
      screen.getByText(/Pull refuses to run until they are committed/),
    ).toBeVisible();
  });

  it("lists the other clone of the same remote and says Toolport leaves it alone", async () => {
    bridge.set("library status", status("duplicate"));
    await open();
    const clones = library().getByRole("list", { name: "Other clones" });
    expect(within(clones).getByText("/fixture/home/lib/ai-skills-copy")).toBeVisible();
    expect(within(clones).getByText("same remote")).toBeVisible();
    expect(
      within(clones).getByText(/never changes or deletes the other one/),
    ).toBeVisible();
  });

  it("turns Pull and Push off with the reason when the library has no remote", async () => {
    bridge.set("library status", status("no-remote"));
    await open();
    expect(library().getByText("No remote")).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(library().getByRole("button", { name: "Push…" })).toBeDisabled();
    expect(library().getByRole("button", { name: "Check the remote" })).toBeDisabled();
    expect(screen.getAllByText(/has no remote/).length).toBeGreaterThan(0);
  });

  it("is a status line while the library is read, with Pull and Push off", async () => {
    let release: () => void = () => {};
    bridge.set(
      "library status",
      () =>
        new Promise((resolve) => {
          release = () => resolve(status("behind"));
        }),
    );
    await open(false);
    expect(await library().findByText("Reading the library status…")).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeDisabled();
    release();
    expect(await library().findByText("Changes here")).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeEnabled();
  });

  it("shows the CLI's error with Retry, and reads again on Retry", async () => {
    bridge.set(
      "library status",
      failure("not_found", "no git repository in the library"),
    );
    const user = await open(false);
    expect(await library().findByText(/no git repository in the library/)).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(library().getByRole("button", { name: "Push…" })).toBeDisabled();
    bridge.set("library status", status("behind"));
    await user.click(library().getByRole("button", { name: "Retry" }));
    expect(await library().findByText("Changes here")).toBeVisible();
    expect(bridge.count("library status")).toBe(2);
  });

  it("survives a bridge that is down, and recovers on Retry", async () => {
    wire({ invoke, listen }, bridge);
    const real = invoke.getMockImplementation()!;
    invoke.mockImplementation((command: string, args: { argv?: string[] }) =>
      command === "plus_ctl" && args.argv?.[0] === "library"
        ? Promise.reject(new Error("the bridge is not running"))
        : real(command, args),
    );
    const user = await open(false);
    expect(await library().findByText(/the bridge is not running/)).toBeVisible();
    invoke.mockImplementation(real);
    await user.click(library().getByRole("button", { name: "Retry" }));
    expect(await library().findByText("Changes here")).toBeVisible();
  });

  it("says it is offline and that the numbers are from the last fetch", async () => {
    const online = vi.spyOn(window.navigator, "onLine", "get").mockReturnValue(false);
    try {
      await open();
      expect(
        screen.getByText(/Check the remote, Pull and Push need the network/),
      ).toBeVisible();
      online.mockReturnValue(true);
      act(() => {
        window.dispatchEvent(new Event("online"));
      });
      await waitFor(() =>
        expect(screen.queryByText(/Pull and Push need the network/)).toBeNull(),
      );
    } finally {
      online.mockRestore();
    }
  });
});

describe("Library row: Check the remote", () => {
  it("reaches the remote only when pressed, then shows what the sign-in did", async () => {
    const user = await open();
    expect(bridge.count("library status --fetch")).toBe(0);
    await user.click(library().getByRole("button", { name: "Check the remote" }));
    expect(
      await library().findByText(/your git credentials; the remote accepted them/),
    ).toBeVisible();
    expect(bridge.count("library status --fetch")).toBe(1);
    expect(bridge.count("library status")).toBe(1);
  });

  it("says the remote could not be reached and keeps the old numbers", async () => {
    const down = status("fetch");
    bridge.set("library status --fetch", {
      ...down,
      fetch: { requested: true, ok: false, error: "could not resolve host" },
      auth: { ...down.auth, ok: false },
    });
    const user = await open();
    await user.click(library().getByRole("button", { name: "Check the remote" }));
    expect(
      await screen.findByText(/The remote could not be reached: could not resolve host/),
    ).toBeVisible();
    expect(library().getByText("2 commits behind, 0 commits ahead")).toBeVisible();
  });

  it("shows an error of the check with Retry, and the panel stays usable", async () => {
    bridge.set("library status --fetch", failure("failed", "git fetch timed out"));
    const user = await open();
    await user.click(library().getByRole("button", { name: "Check the remote" }));
    expect(await library().findByText("Couldn't check the remote")).toBeVisible();
    expect(library().getByText(/git fetch timed out/)).toBeVisible();
    expect(library().getByRole("button", { name: "Pull" })).toBeEnabled();
    bridge.set("library status --fetch", status("fetch"));
    await user.click(library().getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(library().queryByText("Couldn't check the remote")).toBeNull(),
    );
  });
});

const dialog = (name: RegExp) => screen.findByRole("dialog", { name });

describe("Library row: Pull", () => {
  it("previews the commits, writes nothing until Pull is confirmed, then shows the result", async () => {
    const user = await open();
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await dialog(/^Pull from the remote\?$/);
    expect(
      within(box).getByText("Fast-forward 2 commit(s) from origin/main"),
    ).toBeVisible();
    expect(within(box).getByText("628cdc8 Add notes-two")).toBeVisible();
    expect(
      within(box).getByText(/git fetch origin \(refs only, network\)/),
    ).toBeVisible();
    expect(bridge.count("library pull --dry-run")).toBe(1);
    expect(bridge.count("library pull")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Pull" }));
    await screen.findByText("Done");
    expect(screen.getByText("Pulled 2 commits")).toBeVisible();
    expect(screen.getByText(/skills\/notes-one\/SKILL\.md/)).toBeVisible();
    expect(bridge.count("library pull")).toBe(1);
  });

  it("Escape cancels the plan, runs nothing and gives the focus back to Pull", async () => {
    const user = await open();
    const pull = library().getByRole("button", { name: "Pull" });
    await user.click(pull);
    await dialog(/^Pull from the remote\?$/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.count("library pull")).toBe(0);
    await waitFor(() => expect(pull).toHaveFocus());
  });

  it("says Already up to date instead of asking to confirm nothing", async () => {
    bridge.set(
      "library pull --dry-run",
      libraryGolden<LibraryPullData>("library-pull.current"),
    );
    const user = await open();
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await dialog(/^Pull from the remote$/);
    expect(within(box).getByRole("status")).toHaveTextContent(/Already up to date/);
    expect(within(box).queryByRole("button", { name: "Pull" })).toBeNull();
    await user.click(within(box).getAllByRole("button", { name: "Close" }).at(-1)!);
    expect(bridge.count("library pull")).toBe(0);
  });

  it("shows the refusal of a library with changes and offers nothing that writes", async () => {
    bridge.set("library status", status("dirty"));
    bridge.set("library pull --dry-run", goldenFailure("library-pull.dirty-preview"));
    const user = await open();
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await dialog(/^Pull from the remote$/);
    expect(
      await within(box).findByText(
        /2 uncommitted change\(s\) in .*commit or stash them first/,
      ),
    ).toBeVisible();
    expect(within(box).getByText("refused")).toBeVisible();
    expect(within(box).queryByRole("button", { name: "Pull" })).toBeNull();
    expect(bridge.count("library pull")).toBe(0);
  });

  it("shows a failed apply with the CLI's words", async () => {
    bridge.set("library pull", goldenFailure("library-pull.dirty"));
    const user = await open();
    await user.click(library().getByRole("button", { name: "Pull" }));
    const box = await dialog(/^Pull from the remote\?$/);
    await user.click(within(box).getByRole("button", { name: "Pull" }));
    expect(await screen.findByText(/commit or stash them first/)).toBeVisible();
    expect(screen.getByText("Failed")).toBeVisible();
  });
});

describe("Library row: Push", () => {
  it("previews the commits and the checks, then pushes only after Push is confirmed", async () => {
    bridge.set("library status", status("ahead"));
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library\?$/);
    expect(within(box).getByText("Push 1 commit(s) to origin/main")).toBeVisible();
    expect(within(box).getByText("commit 7caa837 Add new-local")).toBeVisible();
    expect(
      within(box).getByText(/audit: 3 skill\(s\), 0 finding\(s\), 0 high/),
    ).toBeVisible();
    expect(within(box).getByText(/git push \(never forced\)/)).toBeVisible();
    expect(bridge.count("library push")).toBe(0);
    await user.click(within(box).getByRole("button", { name: "Push" }));
    await screen.findByText("Done");
    expect(screen.getByText("Pushed to the remote")).toBeVisible();
    expect(bridge.count("library push")).toBe(1);
    expect(bridge.ran().filter((line) => line.startsWith("library push"))).toEqual([
      "library push --dry-run",
      "library push",
    ]);
  });

  it("blocks the push on a secret finding: the finding is listed, nothing can be applied", async () => {
    bridge.set(
      "library push --dry-run",
      libraryGolden<LibraryPushData>("library-push.secret-preview"),
    );
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library is blocked$/);
    expect(within(box).getByRole("alert")).toHaveTextContent(/looks like a secret/);
    const findings = within(box).getByRole("list", { name: "Findings" });
    expect(
      within(findings).getByText(
        "github-token in skills/leaky/SKILL.md:5 (commit 13e1696)",
      ),
    ).toBeVisible();
    expect(within(box).queryByRole("button", { name: "Push" })).toBeNull();
    expect(within(box).getAllByRole("button", { name: "Close" }).length).toBeGreaterThan(
      0,
    );
    expect(bridge.count("library push")).toBe(0);
  });

  it("warns about a high audit finding but lets the push go on", async () => {
    const dry = libraryGolden<LibraryPushData>("library-push.dry-run");
    bridge.set("library push --dry-run", {
      ...dry,
      checks: { ...dry.checks, audit: { ran: true, skills: 3, high: 2, findings: [] } },
    });
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library\?$/);
    expect(within(box).getByText(/2 high-severity audit findings/)).toBeVisible();
    expect(within(box).getByText(/does not stop the push/)).toBeVisible();
    expect(within(box).getByRole("button", { name: "Push" })).toBeEnabled();
  });

  it("says Nothing to push when the dry run has nothing to send", async () => {
    bridge.set(
      "library push --dry-run",
      libraryGolden<LibraryPushData>("library-push.current"),
    );
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library$/);
    expect(within(box).getByRole("status")).toHaveTextContent(/Nothing to push/);
    expect(within(box).queryByRole("button", { name: "Push" })).toBeNull();
    expect(bridge.count("library push")).toBe(0);
  });

  it("shows a refused apply, such as a scan that ran after the preview", async () => {
    bridge.set("library push", goldenFailure("library-push.secret"));
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library\?$/);
    await user.click(within(box).getByRole("button", { name: "Push" }));
    expect(
      await screen.findByText(/secret scan blocked the push: github-token/),
    ).toBeVisible();
    expect(screen.getByText("Failed")).toBeVisible();
  });

  it("never shows the text a scanner matched, only the rule, file and line", async () => {
    const canary = "ghp_CANARYsecretvalue0123456789abcdef";
    const dry = libraryGolden<LibraryPushData>("library-push.secret-preview");
    bridge.set("library push --dry-run", {
      ...dry,
      checks: {
        ...dry.checks,
        builtinScan: {
          count: 1,
          findings: dry.checks.builtinScan.findings.map((f) => ({
            ...f,
            match: canary,
            text: `token = ${canary}`,
          })),
        },
      },
    });
    const user = await open();
    await user.click(library().getByRole("button", { name: "Push…" }));
    const box = await dialog(/^Push the library is blocked$/);
    expect(
      within(box).getAllByText(/github-token in skills\/leaky\/SKILL\.md:5/).length,
    ).toBeGreaterThan(0);
    expect(document.body.textContent).not.toContain(canary);
    expect(JSON.stringify(bridge.ran())).not.toContain(canary);
  });
});
