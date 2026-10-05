import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { closeResult, confirmResult, mountContext, review, visibleText } from "./e2e";
import { CANARY, FOLDER, bundleList, seedProfiles } from "./tabsKit";
import { createBridge, failure, goldenData, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge();
  seedProfiles(bridge);
  wire({ invoke, listen }, bridge);
});
afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(bridge.ran().some((line) => /--home|--reveal|secret/.test(line))).toBe(false);
  expect(bridge.stdins()).toEqual([]);
});

async function open() {
  const user = mountContext("profiles");
  await screen.findByRole("region", { name: "Profile acme-dev" });
  return user;
}

const ranOrder = (...lines: string[]) => {
  const ran = bridge.ran();
  const at = lines.map((line) => ran.indexOf(line));
  expect(
    at.every((i) => i >= 0),
    `${lines.join(" | ")} in ${ran.join(" | ")}`,
  ).toBe(true);
  expect([...at].sort((a, b) => a - b)).toEqual(at);
};
const writes = () =>
  bridge
    .ran()
    .filter(
      (line) =>
        /^context (bundle (add|edit|rm|apply|undo|launch|config --auto)|use)/.test(
          line,
        ) && !line.endsWith("--dry-run"),
    );
const detail = () => within(screen.getByRole("region", { name: "Profile acme-dev" }));

async function press(user: UserEvent, name: string | RegExp) {
  await user.click(screen.getByRole("button", { name }));
}

describe("Profiles: reading", () => {
  it("context.bundle.ls, context.bundle.status: lists the profiles with their server set and where they are applied, and selects the first", async () => {
    await open();
    const list = within(screen.getByRole("list", { name: "Profile list" }));
    const items = list.getAllByRole("listitem");
    expect(items.map((li) => within(li).getByRole("button").textContent)).toEqual([
      expect.stringContaining("acme-dev"),
      expect.stringContaining("broken"),
      expect.stringContaining("default"),
    ]);
    expect(items[0]).toHaveTextContent("applied in 1");
    expect(items[0]).toHaveTextContent("server set acme-dev");
    expect(items[0]).toHaveTextContent("3 skills changed");
    expect(items[1]).toHaveTextContent("cannot be read");
    expect(within(items[0]).getByRole("button")).toHaveAttribute("aria-pressed", "true");
    expect(bridge.ran().filter((line) => /--dry-run|--yes/.test(line))).toEqual([]);
  });

  it("context.bundle.show: shows what the profile hides, pairs it with its server set and lists where it is applied", async () => {
    await open();
    const box = detail();
    expect(box.getByText("off: notes-helper")).toBeVisible();
    expect(box.getByText("off: scratch-*")).toBeVisible();
    expect(box.getByText("name only: long-guide")).toBeVisible();
    expect(box.getByText("tools-pack@tools-market")).toBeVisible();
    expect(box.getByText("left out: **/acme-erp/CLAUDE.md")).toBeVisible();
    expect(box.getByText("~/work/acme-erp/clients/*")).toBeVisible();
    expect(box.getByText("Server set").nextElementSibling).toHaveTextContent("acme-dev");
    const applied = within(box.getByRole("list", { name: "Applied in" }));
    expect(applied.getByText(FOLDER)).toBeVisible();
    expect(applied.getByRole("button", { name: "Undo…" })).toBeEnabled();
  });

  it("shows another profile when it is chosen, with the legacy list marked and Edit off", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: /^default/ }));
    const box = within(await screen.findByRole("region", { name: "Profile default" }));
    expect(box.getByText("legacy skill list")).toBeVisible();
    expect(box.getByRole("button", { name: "Edit" })).toBeDisabled();
    expect(
      box.getByText(/unchanged: the server profile with the same name/),
    ).toBeVisible();
    expect(box.getByText("nowhere yet")).toBeVisible();
    expect(box.getByRole("list", { name: "Problems" })).toHaveTextContent(
      "a plain skills list is read as skills.allow",
    );
  });

  it("never renders the definition file, only the parts of the profile", async () => {
    await open();
    expect(visibleText()).not.toContain(CANARY);
    expect(visibleText()).not.toContain("format: 1");
  });

  it("flags drift where a profile is applied", async () => {
    const data = bundleList();
    data.bundles[0].appliedTo[0].drift = true;
    bridge.set("context bundle ls", data);
    bridge.set("context bundle show acme-dev", () => ({
      ...goldenData("context-bundle-show.bundle"),
      appliedTo: data.bundles[0].appliedTo,
    }));
    await open();
    expect(
      within(screen.getByRole("list", { name: "Applied in" })).getByText(
        "changed since the apply",
      ),
    ).toBeVisible();
  });
});

describe("Profiles: states", () => {
  it("shows a skeleton while the list is read", async () => {
    bridge.set("context bundle ls", () => new Promise(() => {}));
    mountContext("profiles");
    const region = await screen.findByRole("region", { name: "Profiles" });
    expect(within(region).getByRole("status", { name: "Loading" })).toBeVisible();
    expect(bridge.ran().some((line) => line.startsWith("context bundle show"))).toBe(
      false,
    );
  });

  it("offers to create the first profile when there is none", async () => {
    bridge.set("context bundle ls", { bundles: [] });
    mountContext("profiles");
    expect(await screen.findByText(/No profile yet/)).toBeVisible();
    expect(screen.getByRole("button", { name: /Create from a folder/ })).toBeEnabled();
  });

  it("shows the CLI's words with Retry for a failed list and recovers", async () => {
    bridge.set("context bundle ls", failure("io", "cannot read the library"));
    const user = mountContext("profiles");
    const region = within(await screen.findByRole("region", { name: "Profiles" }));
    expect(await region.findByRole("alert")).toHaveTextContent("cannot read the library");
    bridge.set("context bundle ls", bundleList());
    await user.click(region.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("region", { name: "Profile acme-dev" })).toBeVisible();
  });

  it("says when one profile cannot be read and retries only that read", async () => {
    const user = await open();
    await user.click(screen.getByRole("button", { name: /^broken/ }));
    const alert = await screen.findByText(/Couldn't read the profile\./);
    expect(alert).toBeVisible();
    bridge.set("context bundle show broken", () => ({
      ...goldenData("context-bundle-show.bundle"),
      name: "broken",
    }));
    await user.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("region", { name: "Profile broken" })).toBeVisible();
  });

  it("shows every read as failed while toolportctl is down, and starts no write", async () => {
    invoke.mockReset().mockImplementation(async (command: string) => {
      if (command === "plus_ctl") throw new Error("toolportctl was not found");
      throw new Error(`unexpected invoke ${command}`);
    });
    mountContext("profiles");
    await waitFor(() =>
      expect(screen.getAllByText(/toolportctl was not found/).length).toBeGreaterThan(0),
    );
    expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
  });
});

describe("Profiles: creating", () => {
  it("context.bundle.add: creates a profile from a folder through the preview and shows it in the list", async () => {
    const created = goldenData("context-bundle-add.apply");
    const list = bundleList();
    bridge.set(
      `context bundle add from-client --from-folder ${FOLDER} --dry-run`,
      goldenData("context-bundle-add.from-folder"),
    );
    bridge.set(`context bundle add from-client --from-folder ${FOLDER}`, () => {
      bridge.set("context bundle ls", {
        bundles: [...list.bundles, { ...list.bundles[2], name: "from-client" }],
      });
      return created;
    });
    const user = await open();
    await press(user, /Create from a folder/);
    const form = within(
      await screen.findByRole("dialog", { name: "Create a profile from a folder" }),
    );
    expect(form.getByRole("button", { name: "Review the profile" })).toBeDisabled();
    await user.type(form.getByLabelText("Name"), "from-client");
    expect(form.getByRole("button", { name: "Review the profile" })).toBeDisabled();
    await user.type(form.getByLabelText("Folder to read"), FOLDER);
    await user.click(form.getByRole("button", { name: "Review the profile" }));
    const box = await review(/Create profile from-client\?/);
    expect(box.getByText("Add bundle from-client")).toBeVisible();
    expect(writes()).toEqual([]);
    const result = await confirmResult(
      user,
      /Create profile from-client\?/,
      "Create profile",
    );
    expect(result.getByText(created.plan.summary)).toBeVisible();
    expect(result.getByText(/To undo:/)).toBeVisible();
    ranOrder(
      `context bundle add from-client --from-folder ${FOLDER} --dry-run`,
      `context bundle add from-client --from-folder ${FOLDER}`,
    );
    await closeResult(user);
    expect(
      await within(screen.getByRole("list", { name: "Profile list" })).findByText(
        "from-client",
      ),
    ).toBeVisible();
  });

  it("builds a new profile from the lists, one value per line, and refuses a taken name", async () => {
    bridge.set(
      "context bundle add ops-lite --description Operations --servers acme-dev --skills-off scratch-one,scratch-two --plugins-off tools-pack@tools-market --dry-run",
      goldenData("context-bundle-add.preview"),
    );
    const user = await open();
    await press(user, "New profile");
    const form = within(await screen.findByRole("dialog", { name: "New profile" }));
    await user.type(form.getByLabelText("Name"), "acme-dev");
    expect(form.getByText("A profile with this name exists.")).toBeVisible();
    expect(form.getByRole("button", { name: "Review the profile" })).toBeDisabled();
    await user.clear(form.getByLabelText("Name"));
    await user.type(form.getByLabelText("Name"), "-bad");
    expect(
      form.getAllByText("Letters, digits, dot, dash and underscore.").length,
    ).toBeGreaterThan(0);
    await user.clear(form.getByLabelText("Name"));
    await user.type(form.getByLabelText("Name"), "ops-lite");
    await user.type(form.getByLabelText("Description"), "Operations");
    await user.type(form.getByLabelText("Server set"), "acme-dev");
    await user.type(
      form.getByLabelText("Skills turned off"),
      "scratch-one{Enter}scratch-two",
    );
    await user.type(form.getByLabelText("Plugins turned off"), "tools-pack@tools-market");
    await user.click(form.getByRole("button", { name: "Review the profile" }));
    const box = await review(/Create profile ops-lite\?/);
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      "toolportctl context bundle add ops-lite --description Operations --servers acme-dev --skills-off scratch-one,scratch-two --plugins-off tools-pack@tools-market",
    );
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
  });
});

describe("Profiles: editing and duplicating", () => {
  it("context.bundle.edit: sends only the lists that changed, and shows the plan before it saves", async () => {
    const edit = "context bundle edit acme-dev --agents-off reviewer-bot,planner";
    bridge.set(`${edit} --dry-run`, goldenData("context-bundle-edit.preview"));
    bridge.set(edit, goldenData("context-bundle-edit.apply"));
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit profile acme-dev" }),
    );
    expect(form.getByLabelText("Name")).toBeDisabled();
    expect(form.getByRole("button", { name: "Review the changes" })).toBeDisabled();
    await user.type(form.getByLabelText("Agents turned off"), "{Enter}planner");
    await user.click(form.getByRole("button", { name: "Review the changes" }));
    const result = await confirmResult(user, /Change profile acme-dev\?/, "Save changes");
    expect(
      result.getByText(goldenData("context-bundle-edit.apply").plan.summary),
    ).toBeVisible();
    ranOrder(`${edit} --dry-run`, edit);
    await closeResult(user);
  });

  it("clears a list by sending its flag with an empty value", async () => {
    const edit = ["context", "bundle", "edit", "acme-dev", "--plugins-off", ""];
    bridge.set(`${edit.join(" ")} --dry-run`, goldenData("context-bundle-edit.preview"));
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit profile acme-dev" }),
    );
    await user.clear(form.getByLabelText("Plugins turned off"));
    await user.click(form.getByRole("button", { name: "Review the changes" }));
    await review(/Change profile acme-dev\?/);
    const call = invoke.mock.calls.find(
      ([command, args]) =>
        command === "plus_ctl" &&
        (args as { argv: string[] }).argv.includes("--plugins-off"),
    );
    expect((call![1] as { argv: string[] }).argv).toEqual([...edit, "--dry-run"]);
  });

  it("duplicates under a new name with every list of the original", async () => {
    const add =
      "context bundle add acme-dev-copy --description ERP work: no marketplace plugins, only the ERP skills --servers acme-dev --skills-off notes-helper,scratch-* --skills-name-only long-guide --plugins-off tools-pack@tools-market,loop-runner@official --layers-add acme-knowledge --layers-exclude **/acme-erp/CLAUDE.md --agents-off reviewer-bot --bind ~/work/acme-erp/clients/*";
    bridge.set(`${add} --dry-run`, goldenData("context-bundle-add.preview"));
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Duplicate" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Duplicate profile acme-dev" }),
    );
    expect(form.getByLabelText("Name")).toHaveValue("acme-dev-copy");
    await user.click(form.getByRole("button", { name: "Review the profile" }));
    await review(/Create profile acme-dev-copy\?/);
    expect(bridge.ran()).toContain(`${add} --dry-run`);
    expect(writes()).toEqual([]);
  });
});

describe("Profiles: deleting", () => {
  const rm = "context bundle rm acme-dev --force";

  it("context.bundle.rm: needs the force box for a profile that is applied, and the name typed before it deletes", async () => {
    bridge.set(`${rm} --dry-run`, goldenData("context-bundle-rm.forced"));
    bridge.set(rm, goldenData("context-bundle-rm.apply"));
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete profile acme-dev" }),
    );
    expect(form.getByRole("button", { name: "Review the deletion" })).toBeDisabled();
    await user.click(
      form.getByRole("checkbox", { name: /although it is applied in 1 folder/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    const box = await review(/Delete profile acme-dev\?/);
    expect(box.getByText("Remove bundle acme-dev")).toBeVisible();
    const confirm = box.getByRole("button", { name: "Delete profile" });
    expect(confirm).toBeDisabled();
    await user.type(box.getByLabelText(/Type acme-dev to confirm/), "acme");
    expect(confirm).toBeDisabled();
    expect(writes()).toEqual([]);
    await user.type(box.getByLabelText(/Type acme-dev to confirm/), "-dev");
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    await screen.findByRole("region", { name: "Result" });
    ranOrder(`${rm} --dry-run`, rm);
    await closeResult(user);
  });

  it("deletes an unapplied profile without the force box, and Escape cancels the review", async () => {
    const plain = "context bundle rm default";
    bridge.set(`${plain} --dry-run`, goldenData("context-bundle-rm.preview"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: /^default/ }));
    await screen.findByRole("region", { name: "Profile default" });
    await user.click(screen.getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete profile default" }),
    );
    expect(form.queryByRole("checkbox")).toBeNull();
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    await review(/Delete profile default\?/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(bridge.ran()).toContain(`${plain} --dry-run`);
    expect(writes()).toEqual([]);
  });
});

describe("Profiles: applying and undoing", () => {
  it("applies the profile and its server set with context use: plan, confirm, result, then the undo line", async () => {
    const use = goldenData("context-use.bundle");
    const argv = `context use acme-dev --cwd ${FOLDER}`;
    bridge.set(`${argv} --dry-run`, { ...use, dryRun: true, result: null });
    bridge.set(argv, use);
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile acme-dev to a folder" }),
    );
    expect(form.getByRole("button", { name: "Review the plan" })).toBeDisabled();
    await user.type(form.getByLabelText("Folder"), FOLDER);
    await user.click(form.getByRole("button", { name: "Review the plan" }));
    const box = await review(/Apply profile acme-dev to clients\/acme-erp\?/);
    expect(box.getByText(use.plan.summary)).toBeVisible();
    expect(box.getByRole("list", { name: "Changes" })).toBeVisible();
    expect(writes()).toEqual([]);
    const result = await confirmResult(
      user,
      /Apply profile acme-dev to clients\/acme-erp\?/,
      "Apply profile",
    );
    expect(result.getByText(use.plan.summary)).toBeVisible();
    expect(result.getByText(/toolportctl context use --none --cwd/)).toBeVisible();
    ranOrder(`${argv} --dry-run`, argv);
    await closeResult(user);
  });

  it("uses context bundle apply for a profile without a server set", async () => {
    const plan = goldenData("context-bundle-apply.plan");
    const argv = `context bundle apply default --cwd ${FOLDER}`;
    bridge.set(`${argv} --dry-run`, plan);
    bridge.set(argv, goldenData("context-bundle-apply.result"));
    const user = await open();
    await user.click(screen.getByRole("button", { name: /^default/ }));
    await screen.findByRole("region", { name: "Profile default" });
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile default to a folder" }),
    );
    await user.type(form.getByLabelText("Folder"), `${FOLDER}{Enter}`);
    await confirmResult(
      user,
      /Apply profile default to clients\/acme-erp\?/,
      "Apply profile",
    );
    expect(bridge.ran().some((line) => line.startsWith("context use default"))).toBe(
      false,
    );
    ranOrder(`${argv} --dry-run`, argv);
    await closeResult(user);
  });

  it("offers the folders of earlier applies and the recent ones in the folder field", async () => {
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Apply to a folder…" }));
    await screen.findByRole("dialog", { name: "Apply profile acme-dev to a folder" });
    const options = [...document.querySelectorAll("datalist option")].map((option) =>
      option.getAttribute("value"),
    );
    expect(options).toContain(FOLDER);
  });

  it("undoes the profile in a folder: plan, confirm, result", async () => {
    const undo = `context bundle undo --cwd ${FOLDER}`;
    bridge.set(`${undo} --dry-run`, goldenData("context-bundle-undo.plan"));
    bridge.set(undo, goldenData("context-bundle-undo.result"));
    const user = await open();
    await user.click(detail().getByRole("button", { name: "Undo…" }));
    const result = await confirmResult(
      user,
      /Undo profile acme-dev in clients\/acme-erp\?/,
      "Undo",
    );
    expect(
      result.getByText("Undo bundle acme-dev in <WORLD>/home/work/erp/clients/acme-erp"),
    ).toBeVisible();
    ranOrder(`${undo} --dry-run`, undo);
    await closeResult(user);
  });

  it("shows a refused apply in the CLI's words and keeps the screen", async () => {
    const argv = `context bundle apply default --cwd ${FOLDER}`;
    bridge.set(
      `${argv} --dry-run`,
      failure("conflict", "bundle default is applied in another folder"),
    );
    const user = await open();
    await user.click(screen.getByRole("button", { name: /^default/ }));
    await screen.findByRole("region", { name: "Profile default" });
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile default to a folder" }),
    );
    await user.type(form.getByLabelText("Folder"), FOLDER);
    await user.click(form.getByRole("button", { name: "Review the plan" }));
    expect(await screen.findByText(/applied in another folder/)).toBeVisible();
    expect(writes()).toEqual([]);
  });
});

describe("Profiles: apply automatically and launch", () => {
  it("context.bundle.config: is off by default, and turning it on asks first because that command has no preview", async () => {
    bridge.set(
      "context bundle config --auto-apply on",
      goldenData("context-bundle-config.on"),
    );
    const user = await open();
    const toggle = await screen.findByRole("switch", { name: "Apply automatically" });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    const box = await review(/Apply profiles automatically\?/);
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      "toolportctl context bundle config --auto-apply on",
    );
    expect(writes()).toEqual([]);
    bridge.set("context bundle config", goldenData("context-bundle-config.on"));
    await user.click(box.getByRole("button", { name: "Turn on" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("context bundle config --auto-apply on"),
    );
    await closeResult(user);
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Apply automatically" })).toBeChecked(),
    );
  });

  it("context.bundle.launch: prepares the launch line, shows it to copy, and keeps Open in Terminal off", async () => {
    const launch = goldenData("context-bundle-launch.apply");
    bridge.set("context bundle launch acme-dev", launch);
    const user = await open();
    const box = detail();
    expect(box.getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    await user.click(box.getByRole("button", { name: "Prepare the launch line…" }));
    await review(/Prepare the launch line for acme-dev\?/);
    expect(bridge.ran()).not.toContain("context bundle launch acme-dev");
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Write the settings file",
      }),
    );
    const line = within(await screen.findByRole("region", { name: "Launch line" }));
    expect(line.getByText(launch.command)).toBeVisible();
    expect(line.getByRole("button", { name: "Copy" })).toBeVisible();
    await closeResult(user);
    expect(detail().getByText(launch.command)).toBeVisible();
    expect(detail().getByText(/rides CLAUDE\.local\.md/)).toBeVisible();
    expect(detail().getByRole("button", { name: "Open in Terminal" })).toBeDisabled();
    expect(invoke.mock.calls.some(([command]) => command === "open_terminal")).toBe(
      false,
    );
  });
});

describe("Profiles: keyboard", () => {
  it("chooses a profile with Enter and submits the apply form with Enter in the folder field", async () => {
    bridge.set(
      `context bundle apply default --cwd ${FOLDER} --dry-run`,
      goldenData("context-bundle-apply.plan"),
    );
    const user = await open();
    const other = screen.getByRole("button", { name: /^default/ });
    other.focus();
    await user.keyboard("{Enter}");
    await screen.findByRole("region", { name: "Profile default" });
    expect(other).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    await user.type(screen.getByLabelText("Folder"), `${FOLDER}{Enter}`);
    await review(/Apply profile default to clients\/acme-erp\?/);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(writes()).toEqual([]);
  });

  it("keeps focus inside the form dialog and returns to the page on Escape", async () => {
    const user = await open();
    await press(user, "New profile");
    const dialog = await screen.findByRole("dialog", { name: "New profile" });
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
