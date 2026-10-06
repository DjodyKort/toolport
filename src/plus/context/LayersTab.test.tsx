import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { closeResult, confirmResult, mountContext, review, visibleText } from "./e2e";
import {
  CANARY,
  FOLDER,
  OTHER,
  layerList,
  orgSources,
  seedHere,
  seedLayers,
} from "./tabsKit";
import { createBridge, failure, goldenData, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge();
  seedLayers(bridge);
  wire({ invoke, listen }, bridge);
});
afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(bridge.ran().some((line) => /--home|--reveal|secret/.test(line))).toBe(false);
  expect(bridge.stdins()).toEqual([]);
});

async function open() {
  const user = mountContext("layers");
  await screen.findByRole("region", { name: "Layer client-acme" });
  return user;
}

const layerRow = (name: string) =>
  within(screen.getByRole("list", { name: "Layer list" }))
    .getAllByRole("button")
    .find((button) => button.querySelector("b")?.textContent === name)!;

const writes = () =>
  bridge
    .ran()
    .filter(
      (line) => /^context client (add|edit|rm)/.test(line) && !line.endsWith("--dry-run"),
    );

const ranOrder = (...lines: string[]) => {
  const ran = bridge.ran();
  const at = lines.map((line) => ran.indexOf(line));
  expect(
    at.every((i) => i >= 0),
    `${lines.join(" | ")} in ${ran.join(" | ")}`,
  ).toBe(true);
  expect([...at].sort((a, b) => a - b)).toEqual(at);
};

describe("Layers: reading", () => {
  it("lists the layers with their scope, delivery and whether they are deployed", async () => {
    await open();
    const buttons = within(screen.getByRole("list", { name: "Layer list" })).getAllByRole(
      "button",
    );
    expect(buttons.map((b) => b.querySelector("b")?.textContent)).toEqual([
      "client-acme",
      "client-acme-erp",
      "client-acme-kb",
      "client-chain",
      "client-erp-knowledge",
      "client-linked",
      "client-loop",
      "personal",
      "team-conventions",
    ]);
    expect(layerRow("client-acme")).toHaveTextContent(
      "folder pattern **/clients/acme/**",
    );
    expect(layerRow("client-acme")).toHaveTextContent("not deployed");
    expect(layerRow("client-acme-kb")).toHaveTextContent("1 chosen folder(s)");
    expect(layerRow("client-acme-kb")).toHaveTextContent("deployed");
    expect(layerRow("team-conventions")).toHaveTextContent("everywhere");
    expect(layerRow("client-linked")).toHaveTextContent("import");
    expect(layerRow("client-linked")).toHaveTextContent("needs a look");
    expect(layerRow("client-acme")).toHaveAttribute("aria-pressed", "true");
    expect(bridge.ran().filter((line) => /--dry-run|--yes/.test(line))).toEqual([]);
  });

  it("shows the file, scope, folders, imports and delivery of the chosen layer, and its problems", async () => {
    const user = await open();
    await user.click(layerRow("client-linked"));
    const box = within(
      await screen.findByRole("region", { name: "Layer client-linked" }),
    );
    expect(box.getByText("Synthetic client-linked")).toBeVisible();
    expect(box.getByText("an @import line")).toBeVisible();
    expect(box.getByText("~/kb/CLAUDE.md")).toBeVisible();
    expect(box.getByText("~/work/erp/clients/acme-two")).toBeVisible();
    expect(box.getByRole("list", { name: "Layer problems" })).toHaveTextContent(
      /delivery: an import outside the folder Claude starts in is skipped/,
    );
    await user.click(layerRow("client-acme-kb"));
    const kb = within(
      await screen.findByRole("region", { name: "Layer client-acme-kb" }),
    );
    expect(kb.getByText("text copied into the layer")).toBeVisible();
    expect(kb.getByText(/CLAUDE\.local\.md/)).toBeVisible();
  });

  it("shows the org file read-only with its owner and when it was last synced", async () => {
    await open();
    const org = within(await screen.findByRole("region", { name: "Org file" }));
    expect(org.getByText("read-only")).toBeVisible();
    expect(org.getByText("corp-tools")).toBeVisible();
    expect(org.getByText("corp-tools sync (about every 4 h)")).toBeVisible();
    expect(org.getByText("16 tokens, estimate")).toBeVisible();
    const when = org.getByText((_, node) => node?.tagName === "TIME");
    expect(when).toHaveAttribute("datetime", "2026-10-05T04:00:00Z");
    expect(org.getByText(/never edits this file/)).toBeVisible();
    expect(org.queryByRole("button", { name: /Edit|Delete|Save/ })).toBeNull();
    expect(bridge.ran()).toContain("sources ls --source org");
    expect(writes()).toEqual([]);
  });

  it("says there is no org file when the source is missing, without hiding the layers", async () => {
    bridge.set("sources ls --source org", { ...orgSources(), sources: [] });
    await open();
    expect(await screen.findByText("No org file on this computer.")).toBeVisible();
    expect(screen.getByRole("list", { name: "Layer list" })).toBeVisible();
  });

  it("shows a failed org read with Retry beside the layers", async () => {
    bridge.set("sources ls --source org", failure("io", "cannot read the org clone"));
    const user = await open();
    const org = within(await screen.findByRole("region", { name: "Org file" }));
    expect(await org.findByRole("alert")).toHaveTextContent("cannot read the org clone");
    bridge.set("sources ls --source org", orgSources());
    await user.click(org.getByRole("button", { name: "Retry" }));
    expect(await org.findByText("corp-tools sync (about every 4 h)")).toBeVisible();
  });
});

describe("Layers: states", () => {
  it("shows a skeleton while the layers are read", async () => {
    bridge.set("context client list", () => new Promise(() => {}));
    mountContext("layers");
    const region = await screen.findByRole("region", { name: "Layers" });
    expect(within(region).getByRole("status", { name: "Loading" })).toBeVisible();
  });

  it("offers to add the first layer when there is none", async () => {
    bridge.set("context client list", { layers: [] });
    mountContext("layers");
    expect(await screen.findByText(/No layer yet/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Add layer/ })).toBeEnabled();
  });

  it("shows the CLI's words with Retry for a failed list and recovers", async () => {
    bridge.set("context client list", failure("io", "cannot read the skills repository"));
    const user = mountContext("layers");
    const region = within(await screen.findByRole("region", { name: "Layers" }));
    expect(await region.findByRole("alert")).toHaveTextContent(
      "cannot read the skills repository",
    );
    bridge.set("context client list", layerList());
    await user.click(region.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("region", { name: "Layer client-acme" }),
    ).toBeVisible();
  });

  it("shows every read as failed while toolportctl is down", async () => {
    invoke.mockReset().mockImplementation(async (command: string) => {
      if (command === "plus_ctl") throw new Error("toolportctl was not found");
      throw new Error(`unexpected invoke ${command}`);
    });
    mountContext("layers");
    await waitFor(() =>
      expect(
        screen.getAllByText(/toolportctl was not found/).length,
      ).toBeGreaterThanOrEqual(2),
    );
    expect(
      screen.getAllByRole("button", { name: "Retry" }).length,
    ).toBeGreaterThanOrEqual(2);
  });
});

describe("Layers: adding", () => {
  it("adds a folder-pattern layer through the plan and shows it in the list", async () => {
    const preview = goldenData("context-client-add.preview");
    const apply = goldenData("context-client-add.apply");
    const argv =
      "context client add new-one --scope glob --glob **/clients/new/** --delivery copy";
    bridge.set(`${argv} --dry-run`, preview);
    bridge.set(argv, () => {
      bridge.set("context client list", {
        layers: [
          ...layerList().layers,
          {
            ...layerList().layers[0],
            name: "client-new-one",
            globs: ["**/clients/new/**"],
          },
        ],
      });
      return apply;
    });
    const user = await open();
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const form = within(await screen.findByRole("dialog", { name: "Add a layer" }));
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
    await user.type(form.getByLabelText("Name"), "new-one");
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
    await user.type(form.getByLabelText("Folder pattern"), "**/clients/new/**");
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    const box = await review(/Add layer new-one\?/);
    expect(box.getByText(preview.plan.summary)).toBeVisible();
    expect(box.getByRole("list", { name: "Changes" })).toBeVisible();
    expect(box.getByText("Show the change")).toBeVisible();
    expect(writes()).toEqual([]);
    const result = await confirmResult(user, /Add layer new-one\?/, "Add layer");
    expect(result.getByText(apply.plan.summary)).toBeVisible();
    expect(result.getByText(/To undo:/)).toBeVisible();
    ranOrder(`${argv} --dry-run`, argv);
    await closeResult(user);
    expect(await screen.findByRole("button", { name: /client-new-one/ })).toBeVisible();
  });

  it("builds a folder layer from folders and an import, with the import delivery chosen", async () => {
    const argv = `context client add kb --scope folder --folder ${FOLDER} --folder ${OTHER} --import /fixture/kb/CLAUDE.md --delivery import`;
    bridge.set(`${argv} --dry-run`, goldenData("context-client-add.folder-preview"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const form = within(await screen.findByRole("dialog", { name: "Add a layer" }));
    await user.type(form.getByLabelText("Name"), "kb");
    await user.selectOptions(form.getByLabelText("Scope"), "folder");
    expect(form.queryByLabelText("Folder pattern")).toBeNull();
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
    await user.type(form.getByLabelText("Folders"), `${FOLDER}{Enter}${OTHER}`);
    await user.type(form.getByLabelText("Imports"), "/fixture/kb/CLAUDE.md");
    await user.click(
      form.getByRole("checkbox", { name: /Keep the imported text inside the layer/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    const box = await review(/Add layer kb\?/);
    expect(box.getByLabelText("Command line")).toHaveTextContent(`toolportctl ${argv}`);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
  });

  it("refuses a name that maps to an existing layer and a name the CLI would reject", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const form = within(await screen.findByRole("dialog", { name: "Add a layer" }));
    await user.type(form.getByLabelText("Name"), "acme");
    await user.type(form.getByLabelText("Folder pattern"), "**/x/**");
    expect(form.getByText("A layer with this name exists.")).toBeVisible();
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
    await user.clear(form.getByLabelText("Name"));
    await user.type(form.getByLabelText("Name"), "Acme Two");
    expect(
      form.getAllByText("Letters, digits, dot, dash and underscore.").length,
    ).toBeGreaterThan(0);
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
  });

  it("shows a refusal of the preview in the CLI's words and writes nothing", async () => {
    bridge.set(
      "context client add taken --scope glob --glob **/t/** --delivery copy --dry-run",
      failure("usage", "rule name client-taken: already exists"),
    );
    const user = await open();
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const form = within(await screen.findByRole("dialog", { name: "Add a layer" }));
    await user.type(form.getByLabelText("Name"), "taken");
    await user.type(form.getByLabelText("Folder pattern"), "**/t/**");
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    expect(
      await screen.findByText(/rule name client-taken: already exists/),
    ).toBeVisible();
    expect(writes()).toEqual([]);
  });
});

describe("Layers: editing", () => {
  const edit = "context client edit client-chain --delivery import";

  it("context.client-edit: sends only the fields that changed, and needs a change before it can be reviewed", async () => {
    bridge.set(`${edit} --dry-run`, goldenData("context-client-edit.delivery"));
    bridge.set(edit, goldenData("context-client-edit.apply"));
    const user = await open();
    await user.click(layerRow("client-chain"));
    await screen.findByRole("region", { name: "Layer client-chain" });
    await user.click(screen.getByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit layer client-chain" }),
    );
    expect(form.getByLabelText("Name")).toBeDisabled();
    expect(form.getByLabelText("Folders")).toHaveValue("~/work/erp/clients/acme-two");
    expect(form.getByRole("button", { name: "Review the layer" })).toBeDisabled();
    await user.click(
      form.getByRole("checkbox", { name: /Keep the imported text inside the layer/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    const result = await confirmResult(
      user,
      /Change layer client-chain\?/,
      "Save changes",
    );
    expect(
      result.getByText(goldenData("context-client-edit.apply").plan.summary),
    ).toBeVisible();
    ranOrder(`${edit} --dry-run`, edit);
    await closeResult(user);
  });

  it("lets a layer that is not a client layer be edited but not deleted", async () => {
    const user = await open();
    await user.click(layerRow("personal"));
    await screen.findByRole("region", { name: "Layer personal" });
    expect(screen.getByRole("button", { name: "Edit" })).toBeEnabled();
    const del = screen.getByRole("button", { name: "Delete…" });
    expect(del).toBeDisabled();
    expect(del).toHaveAttribute("title", "Only client layers are deleted here");
  });
});

describe("Layers: deleting", () => {
  const rm = "context client rm client-chain";

  it("context.client-rm: asks for the name before it deletes, after the plan, and drops the layer from the list", async () => {
    const preview = goldenData("context-client-rm.preview");
    bridge.set(`${rm} --dry-run`, preview);
    bridge.set(rm, () => {
      bridge.set("context client list", {
        layers: layerList().layers.filter(
          (layer: { name: string }) => layer.name !== "client-chain",
        ),
      });
      return goldenData("context-client-rm.apply");
    });
    const user = await open();
    await user.click(layerRow("client-chain"));
    await screen.findByRole("region", { name: "Layer client-chain" });
    await user.click(screen.getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete layer client-chain" }),
    );
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    const box = await review(/Delete layer client-chain\?/);
    expect(box.getByText(preview.plan.summary)).toBeVisible();
    const confirm = box.getByRole("button", { name: "Delete layer" });
    expect(confirm).toBeDisabled();
    await user.type(box.getByLabelText(/Type client-chain to confirm/), "client-cha");
    expect(confirm).toBeDisabled();
    expect(writes()).toEqual([]);
    await user.type(box.getByLabelText(/Type client-chain to confirm/), "in");
    await user.click(confirm);
    await screen.findByRole("region", { name: "Result" });
    ranOrder(`${rm} --dry-run`, rm);
    await closeResult(user);
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /client-chain/ })).toBeNull(),
    );
  });

  it("cancels with Escape at the review and deletes nothing", async () => {
    bridge.set(`${rm} --dry-run`, goldenData("context-client-rm.preview"));
    const user = await open();
    await user.click(layerRow("client-chain"));
    await user.click(await screen.findByRole("button", { name: "Delete…" }));
    await user.click(
      within(
        await screen.findByRole("dialog", { name: "Delete layer client-chain" }),
      ).getByRole("button", { name: "Review the deletion" }),
    );
    await review(/Delete layer client-chain\?/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
  });
});

describe("Layers: the composed preview", () => {
  it("shows the composed text for a folder, collapsed per part, and reads it again after a write", async () => {
    const argv = "context client edit client-chain --delivery import";
    bridge.set(`${argv} --dry-run`, goldenData("context-client-edit.delivery"));
    bridge.set(argv, goldenData("context-client-edit.apply"));
    const user = await open();
    const preview = within(screen.getByRole("region", { name: "Composed preview" }));
    expect(preview.queryByRole("list", { name: "Composed parts" })).toBeNull();
    await user.type(preview.getByLabelText("Folder"), `${FOLDER}{Enter}`);
    const parts = within(await preview.findByRole("list", { name: "Composed parts" }));
    expect(parts.getAllByRole("listitem")).toHaveLength(3);
    expect(
      parts.getByText(/layers: client-acme-erp, client-erp-knowledge/),
    ).toBeVisible();
    for (const details of document.querySelectorAll("details")) {
      expect((details as HTMLDetailsElement).open).toBe(false);
    }
    expect(bridge.count(`context compose --cwd ${FOLDER}`)).toBe(1);
    await user.click(layerRow("client-chain"));
    await user.click(await screen.findByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit layer client-chain" }),
    );
    await user.click(
      form.getByRole("checkbox", { name: /Keep the imported text inside the layer/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    await confirmResult(user, /Change layer client-chain\?/, "Save changes");
    await closeResult(user);
    await waitFor(() => expect(bridge.count(`context compose --cwd ${FOLDER}`)).toBe(2));
  });

  it("takes a pasted shell-quoted folder as the plain path and lists every folder level", async () => {
    const user = await open();
    const preview = within(screen.getByRole("region", { name: "Composed preview" }));
    await user.click(preview.getByLabelText("Folder"));
    await user.paste(`'${FOLDER}'`);
    expect(preview.getByLabelText("Folder")).toHaveValue(FOLDER);
    await user.keyboard("{Enter}");
    const levels = within(await preview.findByRole("list", { name: "Folder levels" }));
    expect(levels.getAllByText("nothing here").length).toBeGreaterThan(0);
    expect(bridge.count(`context compose --cwd ${FOLDER}`)).toBe(1);
  });

  it("renders only what the screen is meant to show: no org warnings or status text, no stored text", async () => {
    const org = orgSources();
    org.sources[0].warnings = [`token ${CANARY}`];
    org.sources[0].status.detail = `read ${CANARY}`;
    bridge.set("sources ls --source org", org);
    const user = await open();
    await screen.findByText("corp-tools sync (about every 4 h)");
    await user.type(screen.getByLabelText("Folder"), `${FOLDER}{Enter}`);
    await screen.findByRole("list", { name: "Composed parts" });
    expect(visibleText()).not.toContain(CANARY);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    expect(window.localStorage.getItem("toolport.context.recent-folders")).toBe(
      JSON.stringify([FOLDER]),
    );
  });

  it("jumps to This folder from the org card", async () => {
    seedHere(bridge);
    const user = await open();
    await user.click(
      await screen.findByRole("button", { name: "Preview the stack for a folder" }),
    );
    await screen.findByRole("tab", { name: "This folder", selected: true });
  });
});

describe("Layers: keyboard", () => {
  it("chooses a layer with Enter and keeps focus in the form dialog until Escape", async () => {
    const user = await open();
    layerRow("client-acme-kb").focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Layer client-acme-kb" });
    expect(layerRow("client-acme-kb")).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const dialog = await screen.findByRole("dialog", { name: "Add a layer" });
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
  });
});
