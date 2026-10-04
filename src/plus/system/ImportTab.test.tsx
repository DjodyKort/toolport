import { beforeEach, describe, expect, it, vi } from "vitest";
import { screen, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { open } from "./harness";
import { ImportTab } from "./ImportTab";
import { createBridge, failure, golden, wire, type Bridge } from "./testkit";

let bridge: Bridge;

beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

const WILDCARD = "the wildcard cuts a server name; only `server__prefix*` patterns map";

function orphans(rule: string | null) {
  return {
    ...golden("import-rename-refs.preview"),
    orphans: [
      {
        path: "/notes/rules.md",
        reference: "mcp__mcpm_alpha-mock__ec*",
        reason: WILDCARD,
        ...(rule ? { rule } : {}),
      },
    ],
    dead: [
      {
        path: "/notes/old.md",
        reference: "mcp__mcpm_ghost__tool",
        reason: "the server is not in the import",
      },
    ],
  };
}

async function fillRewrite(user: Awaited<ReturnType<typeof open>>["user"]) {
  await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
  await user.type(screen.getByLabelText("Tools file"), "/old/tools.json");
  await user.type(
    screen.getByLabelText("Files or folders to rewrite"),
    "/notes{Enter}/project/.claude",
  );
}

describe("Import tab: mcpm import", () => {
  it("keeps Preview off until a config folder is given", async () => {
    const { user } = await open(<ImportTab />, bridge);
    expect(screen.getByRole("button", { name: "Preview import…" })).toBeDisabled();
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    expect(screen.getByRole("button", { name: "Preview import…" })).toBeEnabled();
  });

  it("previews with the same plan as import mcpm --dry-run, and applies it on confirm", async () => {
    bridge.set("import mcpm /old/mcpm --dry-run", golden("import-mcpm.preview"));
    bridge.set("import mcpm /old/mcpm", golden("import-mcpm.apply"));
    const { user } = await open(<ImportTab />, bridge);
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    await user.click(screen.getByRole("button", { name: "Preview import…" }));
    const box = await screen.findByRole("dialog", { name: "Import from mcpm?" });
    expect(
      await within(box).findByText("Import 2 servers, 1 profile and 0 secrets from mcpm"),
    ).toBeInTheDocument();
    const steps = within(within(box).getByRole("list", { name: "Changes" }))
      .getAllByRole("listitem")
      .map((li) => li.textContent?.replace(/^.*?: /, "").trim());
    const wanted = golden("import-mcpm.preview");
    expect(steps).toHaveLength(wanted.servers.length + wanted.profiles.length);
    for (const row of [...wanted.servers, ...wanted.profiles]) {
      expect(
        within(box).getByText(new RegExp(`${row.id}: ${row.action}`)),
      ).toBeInTheDocument();
    }
    expect(bridge.ran()).toContain("import mcpm /old/mcpm --dry-run");
    expect(bridge.ran()).not.toContain("import mcpm /old/mcpm");
    await user.click(within(box).getByRole("button", { name: "Import" }));
    expect(
      await screen.findByText("Imported 2 servers, 1 profile and 0 secrets from mcpm"),
    ).toBeInTheDocument();
    expect(
      bridge.ran().filter((line) => line.startsWith("import mcpm /old/mcpm")),
    ).toEqual(["import mcpm /old/mcpm --dry-run", "import mcpm /old/mcpm"]);
  });

  it("passes the options to the preview and the apply, and shows rejects as warnings", async () => {
    const argv =
      "import mcpm /old/mcpm --skip-clients --prune-orphans --short-ids /old/ids.json";
    bridge.set(`${argv} --dry-run`, {
      ...golden("import-mcpm.preview"),
      rejects: [{ id: "legacy", reason: "no command to run" }],
      warnings: ["client cursor: config not found"],
    });
    bridge.set(argv, golden("import-mcpm.apply"));
    const { user } = await open(<ImportTab />, bridge);
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    await user.type(screen.getByLabelText("Short ids file (optional)"), "/old/ids.json");
    await user.click(
      screen.getByRole("checkbox", { name: /Do not write client configurations/ }),
    );
    await user.click(screen.getByRole("checkbox", { name: /Also remove entries/ }));
    await user.click(screen.getByRole("button", { name: "Preview import…" }));
    const box = await screen.findByRole("dialog");
    expect(
      await within(box).findByText("Not imported, legacy: no command to run"),
    ).toBeInTheDocument();
    expect(within(box).getByText("client cursor: config not found")).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Import" }));
    await screen.findByText(/^Imported 2 servers/);
    expect(bridge.ran()).toContain(argv);
  });

  it("shows a failed preview with the reason and runs no apply", async () => {
    bridge.set(
      "import mcpm /old/mcpm --dry-run",
      failure("input", "no mcpm config under /old/mcpm"),
    );
    const { user } = await open(<ImportTab />, bridge);
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    await user.click(screen.getByRole("button", { name: "Preview import…" }));
    expect(await screen.findByText("no mcpm config under /old/mcpm")).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("import mcpm /old/mcpm");
  });
});

describe("Import tab: rewrite references", () => {
  it("previews the rewrite per file and reports orphans, wildcard cases and dead references", async () => {
    const argv =
      "import rename-refs /old/mcpm --tools /old/tools.json --paths /notes /project/.claude";
    bridge.set(`${argv} --dry-run`, orphans("deny"));
    bridge.set(argv, orphans("deny"));
    const { user } = await open(<ImportTab />, bridge);
    await fillRewrite(user);
    await user.click(screen.getByRole("button", { name: "Preview rewrite…" }));
    const box = await screen.findByRole("dialog", { name: "Rewrite tool references?" });
    expect(
      await within(box).findByText(/Rewrite 1 reference in 1 file \(1 scanned\)/),
    ).toBeInTheDocument();
    expect(
      within(box).getByText(
        /mcp__mcpm_alpha-mock__ec\* is in a deny rule and was not renamed/,
      ),
    ).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "Rewrite" }));
    await screen.findByText(/^Rewrote 1 reference/);
    await user.click(
      within(screen.getByRole("dialog"))
        .getAllByRole("button", { name: "Close" })
        .at(-1)!,
    );
    const report = await screen.findByRole("group", {
      name: /Orphan report \(from the rewrite\)/,
    });
    const left = within(report).getByRole("list", { name: "Orphans" });
    expect(within(left).getByText("mcp__mcpm_alpha-mock__ec*")).toBeInTheDocument();
    expect(within(left).getByText(WILDCARD)).toBeInTheDocument();
    expect(within(left).getByText("in a deny rule")).toBeInTheDocument();
    expect(
      within(
        within(report).getByRole("list", { name: "References to removed servers" }),
      ).getByText("mcp__mcpm_ghost__tool"),
    ).toBeInTheDocument();
    expect(bridge.ran()).toEqual(expect.arrayContaining([`${argv} --dry-run`, argv]));
  });

  it("keeps the report from the preview when the user only previews", async () => {
    const argv =
      "import rename-refs /old/mcpm --tools /old/tools.json --paths /notes /project/.claude";
    bridge.set(`${argv} --dry-run`, orphans(null));
    const { user } = await open(<ImportTab />, bridge);
    await fillRewrite(user);
    await user.click(screen.getByRole("button", { name: "Preview rewrite…" }));
    const box = await screen.findByRole("dialog");
    await within(box).findByText(/Rewrite 1 reference/);
    await user.click(within(box).getByRole("button", { name: "Cancel" }));
    const report = await screen.findByRole("group", {
      name: /Orphan report \(from the preview\)/,
    });
    expect(within(report).queryByText("in a deny rule")).toBeNull();
    expect(bridge.ran()).not.toContain(argv);
  });

  it("says every reference could be mapped when there are no orphans", async () => {
    const argv =
      "import rename-refs /old/mcpm --tools /old/tools.json --paths /notes /project/.claude";
    bridge.set(`${argv} --dry-run`, golden("import-rename-refs.preview"));
    const { user } = await open(<ImportTab />, bridge);
    await fillRewrite(user);
    await user.click(screen.getByRole("button", { name: "Preview rewrite…" }));
    await user.click(
      within(await screen.findByRole("dialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(
      await screen.findByText("Every reference could be mapped."),
    ).toBeInTheDocument();
  });

  it("keeps Preview off until the folder, the tools file and a path are given", async () => {
    const { user } = await open(<ImportTab />, bridge);
    expect(screen.getByRole("button", { name: "Preview rewrite…" })).toBeDisabled();
    await fillRewrite(user);
    expect(screen.getByRole("button", { name: "Preview rewrite…" })).toBeEnabled();
  });

  it("shows the name map of the importer", async () => {
    bridge.set(
      "import mcpm /old/mcpm --dry-run --tools /old/tools.json --name-map",
      golden("import-mcpm.name-map"),
    );
    const { user } = await open(<ImportTab />, bridge);
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    await user.type(screen.getByLabelText("Tools file"), "/old/tools.json");
    await user.click(screen.getByRole("button", { name: "Show name map" }));
    const list = await screen.findByRole("list", { name: "Name map" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(3);
    expect(within(list).getByText("mcp__mcpm_beta-mock__echo")).toBeInTheDocument();
    expect(within(list).getByText("mcp__toolport__beta_mock__echo")).toBeInTheDocument();
    expect(screen.getByText("Name map (3 tools)")).toBeInTheDocument();
  });

  it("shows why the name map failed with Retry", async () => {
    bridge.set(
      "import mcpm /old/mcpm --dry-run --tools /old/tools.json --name-map",
      failure("input", "the tools file is not valid JSON"),
    );
    const { user } = await open(<ImportTab />, bridge);
    await user.type(screen.getByLabelText("mcpm config folder"), "/old/mcpm");
    await user.type(screen.getByLabelText("Tools file"), "/old/tools.json");
    await user.click(screen.getByRole("button", { name: "Show name map" }));
    expect(
      await screen.findByText("the tools file is not valid JSON"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });
});

describe("Import tab: undo and other tools", () => {
  it("shows the rollback command with copy and a disabled Open in Terminal", async () => {
    const { user } = await open(<ImportTab />, bridge);
    const card = screen.getByRole("group", { name: "Undo" });
    expect(within(card).getByLabelText("Command line")).toHaveTextContent(
      "scripts/cutover/rollback.sh --home ~ --backup <backup>",
    );
    expect(within(card).getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    expect(within(card).getByRole("button", { name: "Copy command" })).toBeEnabled();
    await user.type(
      within(card).getByLabelText("Backup folder (optional)"),
      "/backups/cutover 1",
    );
    expect(within(card).getByLabelText("Command line")).toHaveTextContent(
      "--backup '/backups/cutover 1'",
    );
    expect(bridge.ran().filter((line) => line.includes("rollback"))).toEqual([]);
  });

  it("says importing from other tools is not implemented", async () => {
    await open(<ImportTab />, bridge);
    const card = screen.getByRole("group", { name: "Other tools" });
    expect(within(card).getByText("Not implemented")).toBeInTheDocument();
  });
});
