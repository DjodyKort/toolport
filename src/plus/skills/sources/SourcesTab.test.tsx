import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { SourcesTab } from "./SourcesTab";
import {
  createSourcesBridge,
  failure,
  rootsData,
  summaryData,
  wire,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createSourcesBridge();
  wire({ invoke, listen }, bridge);
});

async function open() {
  const user = userEvent.setup();
  render(<SourcesTab />);
  await screen.findByRole("list", { name: "Sources" });
  return user;
}

const list = () => screen.getByRole("list", { name: "Sources" });
const rowButton = (name: string) =>
  within(list())
    .getAllByRole("button")
    .find((b) => within(b).queryByText(name, { selector: "b" }))!;
const detail = (name: string) => screen.getByRole("region", { name: `Source ${name}` });
const stat = (name: string) => within(screen.getByRole("group", { name }));

describe("Sources tab: the list", () => {
  it("lists every source of the scan with its kind, status and read-only mark", async () => {
    await open();
    const rows = within(list()).getAllByRole("listitem");
    expect(rows).toHaveLength(14);
    const library = rowButton("ai-skills");
    expect(within(library).getByText("library")).toBeInTheDocument();
    expect(within(library).getByText("duplicate clone")).toBeInTheDocument();
    expect(within(library).queryByText("read-only")).toBeNull();
    const org = rowButton("corp-tools");
    expect(within(org).getByText("org")).toBeInTheDocument();
    expect(within(org).getByText("out of date")).toBeInTheDocument();
    expect(within(org).getByText("read-only")).toBeInTheDocument();
    expect(within(rowButton("alpha@market")).getByText("plugin")).toBeInTheDocument();
  });

  it("says behind its remote with the commit count, and why", async () => {
    const user = await open();
    const odh = rowButton("odh");
    expect(within(odh).getByText("behind its remote (114)")).toBeInTheDocument();
    await user.click(odh);
    const box = detail("odh");
    expect(within(box).getByText(/114 behind, 0 ahead/)).toBeInTheDocument();
    expect(within(box).getByText(/not in your checkout yet/)).toBeInTheDocument();
    expect(within(box).getByText(/exist only on the remote branch/)).toBeInTheDocument();
    expect(within(box).getByText("read-only")).toBeInTheDocument();
  });

  it("shows owner, place, cost, how it is found and the managing job of one source", async () => {
    const user = await open();
    await user.click(rowButton("corp-tools"));
    const box = detail("corp-tools");
    expect(within(box).getByText("Your organisation")).toBeInTheDocument();
    expect(within(box).getByText("/fixture/home/.local/share/corp-tools")).toBeVisible();
    expect(within(box).getByText(/about 16 tokens \(estimate\)/)).toBeInTheDocument();
    expect(within(box).getByText(/File hashes against the organisation/)).toBeVisible();
    expect(within(box).getByText("2 commands · 1 CLAUDE.md file")).toBeInTheDocument();
    expect(
      within(box).getByText(/shadows the library skill|shadowed/),
    ).toBeInTheDocument();
  });

  it("never shows a token number without saying what it is", async () => {
    await open();
    expect(stat("Cost if all loaded").getByText("tokens, estimate")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(rowButton("odh"));
    expect(within(detail("odh")).getByText(/\(estimate\)/)).toBeInTheDocument();
  });

  it("counts what Claude can see: n of m skills, with the hidden ones as a reason", async () => {
    await open();
    expect(stat("Visible to Claude").getByText("18 of 19")).toBeInTheDocument();
    expect(stat("Visible to Claude").getByText(/1 skill slash-only/)).toBeInTheDocument();
    expect(stat("Needs a look").getByText(/1 behind its remote/)).toBeInTheDocument();
    expect(
      stat("Needs a look").getByText(/1 skills hidden from Claude/),
    ).toBeInTheDocument();
    expect(stat("Sources").getByText("13")).toBeInTheDocument();
  });

  it("shows the visible count of one source and marks the library as editable", async () => {
    const user = await open();
    await user.click(rowButton("ai-skills"));
    const box = detail("ai-skills");
    expect(within(box).getByText("3 of 4 skills")).toBeInTheDocument();
    expect(within(box).getByText("Toolport can edit")).toBeInTheDocument();
    expect(within(box).getByText(/another clone of the same remote/)).toBeInTheDocument();
    expect(
      within(box).getByText(/1 other clone\(s\) of the same remote/),
    ).toBeInTheDocument();
  });

  it("marks a plugin that is switched off and a source without a folder", async () => {
    const user = await open();
    await user.click(rowButton("beta@market"));
    expect(within(detail("beta@market")).getByText("off in your settings")).toBeVisible();
    await user.click(rowButton("_claude"));
    expect(within(detail("_claude")).getByText("no folder")).toBeVisible();
  });

  it("lists the GitHub skills account as planned, with sign-in off", async () => {
    const user = await open();
    await user.click(
      within(list()).getByRole("button", { name: /GitHub skills account/ }),
    );
    const box = screen.getByRole("region", { name: "Source GitHub skills account" });
    expect(
      within(box).getByRole("button", { name: "Sign in with GitHub" }),
    ).toBeDisabled();
    expect(within(box).getByText(/needs MIG-SRC-3/)).toBeInTheDocument();
    expect(bridge.ran().some((argv) => argv.includes("library"))).toBe(false);
  });
});

describe("Sources tab: the library row", () => {
  it("draws Pull and Push off, with the reason, because library commands are not built", async () => {
    const user = await open();
    await user.click(rowButton("ai-skills"));
    const box = detail("ai-skills");
    const pull = within(box).getByRole("button", { name: "Pull" });
    const push = within(box).getByRole("button", { name: "Push…" });
    expect(pull).toBeDisabled();
    expect(push).toBeDisabled();
    expect(pull).toHaveAttribute("title", "needs MIG-SRC-3");
    expect(
      within(box).getByText(/Pull and Push are needs MIG-SRC-3/),
    ).toBeInTheDocument();
    expect(bridge.ran().filter((argv) => argv.startsWith("library"))).toEqual([]);
  });

  it("shows no Pull or Push on another source", async () => {
    const user = await open();
    await user.click(rowButton("corp-tools"));
    expect(
      within(detail("corp-tools")).queryByRole("button", { name: "Pull" }),
    ).toBeNull();
  });

  it("lists the actions no command backs as disabled, with their reasons", async () => {
    const user = await open();
    await user.click(rowButton("~/.claude"));
    const box = detail("~/.claude");
    expect(
      within(box).getByRole("button", { name: "Adopt into library…" }),
    ).toBeDisabled();
    expect(
      within(
        within(box).getByRole("list", { name: "Actions not available yet" }),
      ).getByText(/No command copies a loose file/),
    ).toBeInTheDocument();
  });
});

describe("Sources tab: items of a source", () => {
  it("reads the items only when asked, and lists kind, size, laziness and shadowing", async () => {
    const user = await open();
    await user.click(rowButton("ai-skills"));
    expect(bridge.count("sources ls --source library --items")).toBe(0);
    await user.click(
      within(detail("ai-skills")).getByRole("button", { name: "Show items" }),
    );
    const items = await within(detail("ai-skills")).findByRole("list", { name: "Items" });
    expect(within(items).getAllByRole("listitem")).toHaveLength(6);
    expect(bridge.count("sources ls --source library --items")).toBe(1);
    const shadowed = within(items)
      .getAllByRole("listitem")
      .find((li) => within(li).queryByText("odoo-upgrade", { selector: "b" }))!;
    expect(
      within(shadowed).getByText(/Shares its name with org:command:odoo-upgrade/),
    ).toBeVisible();
    expect(within(shadowed).getAllByText("loads on demand").length).toBeGreaterThan(0);
    await user.click(
      within(detail("ai-skills")).getByRole("button", { name: "Hide items" }),
    );
    expect(within(detail("ai-skills")).queryByRole("list", { name: "Items" })).toBeNull();
  });

  it("shows the CLI's words when the items cannot be read", async () => {
    bridge.set(
      "sources ls --source org --items",
      failure("sources", "the clone is locked"),
    );
    const user = await open();
    await user.click(rowButton("corp-tools"));
    await user.click(
      within(detail("corp-tools")).getByRole("button", { name: "Show items" }),
    );
    expect(await screen.findByText(/the clone is locked/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});

describe("Sources tab: states", () => {
  it("is a skeleton while the scan runs, then the list", async () => {
    let release: () => void = () => {};
    bridge.set(
      "sources ls",
      () =>
        new Promise((resolve) => {
          release = () => resolve(summaryData());
        }),
    );
    render(<SourcesTab />);
    expect(await screen.findByRole("status", { name: "Loading" })).toBeInTheDocument();
    release();
    expect(await screen.findByRole("list", { name: "Sources" })).toBeInTheDocument();
  });

  it("explains an empty scan and offers a rescan", async () => {
    bridge.set("sources ls", { ...summaryData(), sources: [] });
    const user = userEvent.setup();
    render(<SourcesTab />);
    expect(await screen.findByText("No sources found")).toBeInTheDocument();
    bridge.set("sources ls --refresh", summaryData());
    await user.click(screen.getAllByRole("button", { name: "Rescan sources" }).at(-1)!);
    expect(await screen.findByRole("list", { name: "Sources" })).toBeInTheDocument();
    expect(bridge.count("sources ls --refresh")).toBe(1);
  });

  it("shows the CLI's words and a Retry when the scan fails", async () => {
    let calls = 0;
    bridge.set("sources ls", () =>
      ++calls === 1 ? failure("sources", "cannot read context.json") : summaryData(),
    );
    const user = userEvent.setup();
    render(<SourcesTab />);
    expect(await screen.findByText(/cannot read context.json/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("list", { name: "Sources" })).toBeInTheDocument();
  });

  it("names the detector that stopped early instead of showing a silent partial list", async () => {
    bridge.set("sources ls", {
      ...summaryData(),
      partial: true,
      skipped: [{ detector: "repo", reason: "time budget exhausted" }],
    });
    await open();
    const skipped = screen.getByRole("list", { name: "Skipped detectors" });
    expect(within(skipped).getByText("repo")).toBeInTheDocument();
    expect(within(skipped).getByText(/time budget exhausted/)).toBeInTheDocument();
    expect(screen.getByText(/partial: 1 detector stopped early/)).toBeInTheDocument();
  });

  it("rescans with --refresh and reads the list again", async () => {
    const user = await open();
    expect(bridge.count("sources ls")).toBe(1);
    await user.click(screen.getByRole("button", { name: "Rescan sources" }));
    await screen.findByRole("list", { name: "Sources" });
    expect(bridge.count("sources ls --refresh")).toBe(1);
    await user.click(screen.getByRole("button", { name: "Rescan sources" }));
    await waitFor(() => expect(bridge.count("sources ls --refresh")).toBe(2));
  });

  it("says it is offline, and that the list is still complete", async () => {
    const online = vi.spyOn(window.navigator, "onLine", "get").mockReturnValue(false);
    try {
      await open();
      expect(
        screen.getByText(/You are offline\. Scanning reads local files/),
      ).toBeVisible();
      online.mockReturnValue(true);
      act(() => {
        window.dispatchEvent(new Event("online"));
      });
      await waitFor(() => expect(screen.queryByText(/You are offline/)).toBeNull());
    } finally {
      online.mockRestore();
    }
  });

  it("marks a remote that cannot be reached", async () => {
    const data = summaryData();
    data.sources = data.sources.map((row) =>
      row.id === "repo:odh"
        ? {
            ...row,
            status: {
              ...row.status,
              state: "unreachable",
              detail: "origin did not answer",
            },
          }
        : row,
    );
    bridge.set("sources ls", data);
    const user = await open();
    expect(within(rowButton("odh")).getByText("remote unreachable")).toBeInTheDocument();
    await user.click(rowButton("odh"));
    expect(within(detail("odh")).getByText("origin did not answer")).toBeInTheDocument();
    expect(stat("Needs a look").getByText(/1 remote unreachable/)).toBeInTheDocument();
  });
});

describe("Sources tab: the scanned folders", () => {
  it("lists the folders, with Remove only on the one you added", async () => {
    await open();
    const folders = await screen.findByRole("list", { name: "Scanned folders" });
    const items = within(folders).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(within(items[0]).getByText("default")).toBeInTheDocument();
    expect(within(items[0]).getByText("git checkout")).toBeInTheDocument();
    expect(within(items[0]).queryByRole("button", { name: /Stop scanning/ })).toBeNull();
    expect(within(items[0]).getByText(/cannot be removed here/)).toBeInTheDocument();
    expect(within(items[2]).getByText("added by you")).toBeInTheDocument();
    expect(
      within(items[2]).getByRole("button", { name: "Stop scanning dups" }),
    ).toBeEnabled();
  });

  it("marks a folder that is gone", async () => {
    const roots = rootsData().roots.map((r, i) =>
      i === 2 ? { ...r, exists: false } : r,
    );
    bridge.set("sources root ls", { roots });
    await open();
    expect(await screen.findByText("not found")).toBeInTheDocument();
  });

  it("offers the first folder when none is set, and a Retry when listing fails", async () => {
    bridge.set("sources root ls", { roots: [] });
    await open();
    expect(await screen.findByText("No folders to scan yet")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a folder to scan" })).toBeEnabled();
  });

  it("shows the CLI's words when the folders cannot be listed", async () => {
    bridge.set("sources root ls", failure("sources", "context.json is not valid JSON"));
    await open();
    expect(await screen.findByText(/context.json is not valid JSON/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
  });
});

describe("Sources tab: never sends a home", () => {
  it("reads and previews without --home or --data-dir", async () => {
    const user = await open();
    await user.click(rowButton("ai-skills"));
    await user.click(
      within(detail("ai-skills")).getByRole("button", { name: "Show items" }),
    );
    await screen.findByRole("list", { name: "Items" });
    expect(bridge.ran().filter((argv) => /--home|--data-dir/.test(argv))).toEqual([]);
    expect(bridge.missing).toEqual([]);
  });
});
