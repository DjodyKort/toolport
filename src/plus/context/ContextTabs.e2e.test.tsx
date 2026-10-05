import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { closeResult, confirmResult, mountContext, review } from "./e2e";
import { FOLDER, orgSources } from "./tabsKit";
import { createBridge, wire, type Bridge } from "./testkit";

/** The tabs This folder and Profiles walked the way a person uses them, against the world that
 * changes: a preview never changes it, an apply does, and the next read shows it. Each test is
 * named by the parity actions it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createBridge({ world: true });
  bridge.set("sources ls --source org", orgSources());
  wire({ invoke, listen }, bridge);
});
afterEach(() => {
  expect(bridge.missing).toEqual([]);
  expect(bridge.ran().some((line) => /--home|--reveal|secret/.test(line))).toBe(false);
  expect(bridge.stdins()).toEqual([]);
});

const status = () =>
  (
    bridge.world!.run(["context", "bundle", "status", "--cwd", FOLDER]) as {
      applied: { bundle: string } | null;
    }
  ).applied;

async function showFolder(user: UserEvent) {
  await screen.findByRole("group", { name: "Skill list budget" });
  await user.type(screen.getByLabelText("Folder"), FOLDER);
  await user.click(screen.getByRole("button", { name: "Show" }));
  await waitFor(() =>
    expect(screen.getByText(FOLDER, { selector: "code" })).toBeVisible(),
  );
  await screen.findByRole("group", { name: "Skill list budget" });
}

async function backHere(user: UserEvent) {
  await user.click(screen.getByRole("tab", { name: "This folder" }));
  expect(await screen.findByLabelText("Folder")).toHaveValue(FOLDER);
  await waitFor(() =>
    expect(screen.getByText(FOLDER, { selector: "code" })).toBeVisible(),
  );
  await screen.findByRole("group", { name: "Skill list budget" });
}

const numbers = () =>
  screen.getByRole("group", { name: "What loads, in numbers" }).textContent;
const summary = () =>
  within(screen.getByRole("group", { name: "What loads, in numbers" }));

describe("Context tabs against the changing world", () => {
  it("context.measure, context.loads: measure: confirm, progress, result", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const loads = `context loads --cwd ${FOLDER} --measured`;
    const argv = `context measure --cwd ${FOLDER} --yes`;
    await user.click(screen.getByRole("button", { name: "Measure for real…" }));
    const box = await review(/Measure what Claude really loads here\?/);
    expect(box.getByText(/spends model tokens/)).toBeVisible();
    expect(bridge.ran()).not.toContain(argv);
    await user.click(box.getByRole("button", { name: "Measure" }));
    const result = within(await screen.findByRole("region", { name: "Measured" }));
    expect(result.getByText(/Claude Code 2\.1\.289/)).toBeVisible();
    expect(bridge.ran()).toContain(argv);
    await closeResult(user);
    await waitFor(() => expect(bridge.count(loads)).toBe(2));
    expect(
      await summary().findByText(/first request · Claude Code 2\.1\.289/),
    ).toBeVisible();
  });

  it("context.measure-without: measures one plugin out, and shows the saving only from the run", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const plugins = within(screen.getByRole("region", { name: "Plugins" }));
    await user.click(plugins.getByRole("button", { name: "Measure without it…" }));
    const box = await review(/Measure without kit@market\?/);
    expect(bridge.ran().some((line) => line.startsWith("context measure"))).toBe(false);
    await user.click(box.getByRole("button", { name: "Measure" }));
    expect(bridge.ran()).toContain(
      `context measure --cwd ${FOLDER} --without plugin:kit@market --yes`,
    );
    const savings = within(await screen.findByRole("list", { name: "Measured savings" }));
    expect(savings.getAllByRole("listitem").length).toBeGreaterThan(0);
    await closeResult(user);
  });

  it("context.use, context.bundle.apply, context.bundle.undo, context.bundle.status, context.bundle.ls: apply a profile: plan, confirm, result, undo", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const before = numbers();
    expect(status()).toBeNull();

    await user.click(screen.getByRole("tab", { name: "Profiles" }));
    await screen.findByRole("region", { name: "Profile acme-dev" });
    await user.click(screen.getByRole("button", { name: "Apply to a folder…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Apply profile acme-dev to a folder" }),
    );
    const field = form.getByLabelText("Folder") as HTMLInputElement;
    if (!field.value) await user.type(field, FOLDER);
    await user.click(form.getByRole("button", { name: "Review the plan" }));
    const box = await review(/Apply profile acme-dev to clients\/acme-erp\?/);
    expect(box.getByText(/Apply bundle acme-dev in/)).toBeVisible();
    expect(box.getByRole("list", { name: "Changes" })).toBeVisible();
    expect(status()).toBeNull();
    const result = await confirmResult(
      user,
      /Apply profile acme-dev to clients\/acme-erp\?/,
      "Apply profile",
    );
    expect(result.getByText(/toolportctl context use --none --cwd/)).toBeVisible();
    await closeResult(user);
    expect(status()).toMatchObject({ bundle: "acme-dev" });
    const ran = bridge.ran();
    expect(ran.indexOf(`context use acme-dev --cwd ${FOLDER} --dry-run`)).toBeLessThan(
      ran.indexOf(`context use acme-dev --cwd ${FOLDER}`),
    );

    await backHere(user);
    await waitFor(() => expect(numbers()).not.toBe(before));
    await user.click(screen.getByRole("tab", { name: "Profiles" }));
    await waitFor(() =>
      expect(
        within(screen.getByRole("region", { name: "Profile acme-dev" })).getAllByText(
          /clients\/acme-erp/,
        ).length,
      ).toBeGreaterThan(0),
    );
    await user.click(
      within(screen.getByRole("region", { name: "Profile acme-dev" })).getByRole(
        "button",
        {
          name: "Undo…",
        },
      ),
    );
    await confirmResult(user, /Undo profile acme-dev in clients\/acme-erp\?/, "Undo");
    await closeResult(user);
    expect(status()).toBeNull();
    expect(bridge.ran()).toContain(`context bundle undo --cwd ${FOLDER}`);
    await backHere(user);
    await waitFor(() => expect(numbers()).toBe(before));
  });

  it("context.bundle.apply: a profile without a server set goes through context bundle apply", async () => {
    const user = mountContext("profiles");
    await screen.findByRole("region", { name: "Profile acme-dev" });
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
    await closeResult(user);
    expect(status()).toMatchObject({ bundle: "default" });
    expect(bridge.ran().some((line) => line.startsWith("context use default"))).toBe(
      false,
    );
  });

  it("context.compose: shows the composed text of the folder from the layers that match", async () => {
    const user = mountContext("here");
    await showFolder(user);
    const parts = await screen.findByRole("list", { name: "Composed parts" });
    expect(within(parts).getByText("CLAUDE.local.md")).toBeVisible();
    expect(bridge.ran()).toContain(`context compose --cwd ${FOLDER}`);
  });
});

type World = NonNullable<Bridge["world"]>;
const read = <T,>(argv: string[]) => (bridge.world as World).run(argv) as T;
interface BundleRow {
  name: string;
  agents: { off: string[] };
  skills: { off: number };
}
interface LayerRow {
  name: string;
  delivery: string;
  globs: string[];
}
const bundleNamed = (name: string) =>
  read<{ bundles: BundleRow[] }>(["context", "bundle", "ls"]).bundles.find(
    (one) => one.name === name,
  );
const layerNamed = (name: string) =>
  read<{ layers: LayerRow[] }>(["context", "client", "list"]).layers.find(
    (one) => one.name === name,
  );
const profiles = () => within(screen.getByRole("list", { name: "Profile list" }));
const layers = () => within(screen.getByRole("list", { name: "Layer list" }));
const detail = (name: string) =>
  within(screen.getByRole("region", { name: `Profile ${name}` }));

async function openProfiles() {
  const user = mountContext("profiles");
  await screen.findByRole("region", { name: "Profile acme-dev" });
  return user;
}

async function openLayers() {
  const user = mountContext("layers");
  const list = await screen.findByRole("list", { name: "Layer list" });
  await user.click(
    within(list)
      .getAllByRole("button")
      .find((button) => button.querySelector("b")?.textContent === "client-acme")!,
  );
  await screen.findByRole("region", { name: "Layer client-acme" });
  return user;
}

describe("Profiles against the changing world", () => {
  it("context.bundle.add, context.bundle.show: a new profile: the preview changes nothing, confirming adds it to the next list", async () => {
    const user = await openProfiles();
    await user.click(screen.getByRole("button", { name: "New profile" }));
    const form = within(await screen.findByRole("dialog", { name: "New profile" }));
    await user.type(form.getByLabelText("Name"), "ops-lite");
    await user.type(form.getByLabelText("Description"), "Operations");
    await user.type(
      form.getByLabelText("Skills turned off"),
      "scratch-one{Enter}scratch-two",
    );
    await user.click(form.getByRole("button", { name: "Review the profile" }));
    const box = await review(/Create profile ops-lite\?/);
    expect(box.getByText("Add bundle ops-lite")).toBeVisible();
    expect(box.getByRole("list", { name: "Changes" })).toBeVisible();
    expect(bundleNamed("ops-lite")).toBeUndefined();
    const argv =
      "context bundle add ops-lite --description Operations --skills-off scratch-one,scratch-two";
    expect(bridge.ran()).toContain(`${argv} --dry-run`);
    expect(bridge.ran()).not.toContain(argv);
    const result = await confirmResult(
      user,
      /Create profile ops-lite\?/,
      "Create profile",
    );
    expect(result.getByText(/To undo:/)).toBeVisible();
    await closeResult(user);
    expect(bundleNamed("ops-lite")).toMatchObject({ skills: { off: 2 } });
    await user.click(await profiles().findByRole("button", { name: /^ops-lite/ }));
    expect(await detail("ops-lite").findByText("off: scratch-two")).toBeVisible();
    const ran = bridge.ran();
    expect(ran.indexOf(`${argv} --dry-run`)).toBeLessThan(ran.indexOf(argv));
  });

  it("context.bundle.add: from a folder reads what is applied there, and the copy lists the same", async () => {
    read(["context", "use", "acme-dev", "--cwd", FOLDER]);
    const user = await openProfiles();
    await user.click(screen.getByRole("button", { name: /Create from a folder/ }));
    const form = within(
      await screen.findByRole("dialog", { name: "Create a profile from a folder" }),
    );
    await user.type(form.getByLabelText("Name"), "from-client");
    await user.type(form.getByLabelText("Folder to read"), FOLDER);
    await user.click(form.getByRole("button", { name: "Review the profile" }));
    await review(/Create profile from-client\?/);
    expect(bundleNamed("from-client")).toBeUndefined();
    await confirmResult(user, /Create profile from-client\?/, "Create profile");
    await closeResult(user);
    expect(bundleNamed("from-client")).toMatchObject({ skills: { off: 2 } });
    expect(bridge.ran()).toContain(
      `context bundle add from-client --from-folder ${FOLDER}`,
    );
    await user.click(await profiles().findByRole("button", { name: /^from-client/ }));
    expect(await detail("from-client").findByText("off: skill-03")).toBeVisible();
  });

  it("context.bundle.edit: changes one list after a plan and the next read shows it", async () => {
    const user = await openProfiles();
    await user.click(detail("acme-dev").getByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit profile acme-dev" }),
    );
    await user.type(form.getByLabelText("Agents turned off"), "{Enter}planner");
    await user.click(form.getByRole("button", { name: "Review the changes" }));
    const box = await review(/Change profile acme-dev\?/);
    expect(box.getByText("Edit bundle acme-dev")).toBeVisible();
    expect(bundleNamed("acme-dev")!.agents.off).toEqual(["reviewer"]);
    await confirmResult(user, /Change profile acme-dev\?/, "Save changes");
    await closeResult(user);
    expect(bundleNamed("acme-dev")!.agents.off).toEqual(["reviewer", "planner"]);
    const argv = "context bundle edit acme-dev --agents-off reviewer,planner";
    expect(bridge.ran()).toEqual(expect.arrayContaining([`${argv} --dry-run`, argv]));
    await waitFor(() => expect(detail("acme-dev").getByText("planner")).toBeVisible());
  });

  it("context.bundle.rm: deleting an unapplied profile needs its name typed and drops it from the next list", async () => {
    const user = await openProfiles();
    await user.click(profiles().getByRole("button", { name: /^default/ }));
    await screen.findByRole("region", { name: "Profile default" });
    await user.click(detail("default").getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete profile default" }),
    );
    expect(form.queryByRole("checkbox")).toBeNull();
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    const box = await review(/Delete profile default\?/);
    expect(box.getByText("Remove bundle default")).toBeVisible();
    expect(bundleNamed("default")).toBeDefined();
    await confirmResult(user, /Delete profile default\?/, "Delete profile", "default");
    await closeResult(user);
    expect(bundleNamed("default")).toBeUndefined();
    expect(bridge.ran()).toContain("context bundle rm default");
    await waitFor(() =>
      expect(profiles().queryByRole("button", { name: /^default/ })).toBeNull(),
    );
  });

  it("context.bundle.rm: a profile that is applied is only deleted with the force box", async () => {
    read(["context", "use", "acme-dev", "--cwd", FOLDER]);
    const user = await openProfiles();
    await user.click(detail("acme-dev").getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete profile acme-dev" }),
    );
    expect(form.getByRole("button", { name: "Review the deletion" })).toBeDisabled();
    await user.click(
      form.getByRole("checkbox", { name: /although it is applied in 1 folder/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    await confirmResult(user, /Delete profile acme-dev\?/, "Delete profile", "acme-dev");
    await closeResult(user);
    expect(bundleNamed("acme-dev")).toBeUndefined();
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "context bundle rm acme-dev --force --dry-run",
        "context bundle rm acme-dev --force",
      ]),
    );
  });

  it("context.bundle.launch: writes the settings file only after the confirm and shows the line to copy", async () => {
    const user = await openProfiles();
    await user.click(
      detail("acme-dev").getByRole("button", { name: "Prepare the launch line…" }),
    );
    await review(/Prepare the launch line for acme-dev\?/);
    expect(bridge.ran()).not.toContain("context bundle launch acme-dev");
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Write the settings file",
      }),
    );
    const line = within(await screen.findByRole("region", { name: "Launch line" }));
    expect(
      line.getByText(/^claude --settings .*acme-dev\.settings\.json$/),
    ).toBeVisible();
    expect(line.getByRole("button", { name: "Copy" })).toBeVisible();
    await closeResult(user);
    expect(
      detail("acme-dev").getByRole("button", { name: "Open in Terminal" }),
    ).toBeDisabled();
    expect(invoke.mock.calls.some(([command]) => command === "open_terminal")).toBe(
      false,
    );
    expect(status()).toBeNull();
  });

  it("context.bundle.config: apply automatically starts off, asks before it turns on and off, and keeps the switch", async () => {
    const user = await openProfiles();
    const toggle = await screen.findByRole("switch", { name: "Apply automatically" });
    expect(toggle).not.toBeChecked();
    expect(read<{ autoApply: boolean }>(["context", "bundle", "config"]).autoApply).toBe(
      false,
    );
    await user.click(toggle);
    const box = await review(/Apply profiles automatically\?/);
    expect(box.getByLabelText("Command line")).toHaveTextContent(
      "toolportctl context bundle config --auto-apply on",
    );
    expect(bridge.ran()).not.toContain("context bundle config --auto-apply on");
    await user.click(box.getByRole("button", { name: "Turn on" }));
    await closeResult(user);
    expect(read<{ autoApply: boolean }>(["context", "bundle", "config"]).autoApply).toBe(
      true,
    );
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Apply automatically" })).toBeChecked(),
    );
    await user.click(screen.getByRole("switch", { name: "Apply automatically" }));
    await review(/Stop applying profiles automatically\?/);
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "Turn off" }),
    );
    await closeResult(user);
    await waitFor(() =>
      expect(
        screen.getByRole("switch", { name: "Apply automatically" }),
      ).not.toBeChecked(),
    );
    expect(read<{ autoApply: boolean }>(["context", "bundle", "config"]).autoApply).toBe(
      false,
    );
  });
});

describe("Layers against the changing world", () => {
  it("context.client-add, context.client-list: a folder-pattern layer: plan, confirm, and the next list shows it with its pattern", async () => {
    const user = mountContext("layers");
    await screen.findByRole("list", { name: "Layer list" });
    await user.click(screen.getByRole("button", { name: /Add layer/ }));
    const form = within(await screen.findByRole("dialog", { name: "Add a layer" }));
    await user.type(form.getByLabelText("Name"), "partner");
    await user.type(form.getByLabelText("Folder pattern"), "**/work/partner/**");
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    const box = await review(/Add layer partner\?/);
    expect(box.getByText("Add the client layer partner")).toBeVisible();
    expect(layerNamed("client-partner")).toBeUndefined();
    await confirmResult(user, /Add layer partner\?/, "Add layer");
    await closeResult(user);
    expect(layerNamed("client-partner")).toMatchObject({ globs: ["**/work/partner/**"] });
    expect(await layers().findByRole("button", { name: /client-partner/ })).toBeVisible();
    const argv =
      "context client add partner --scope glob --glob **/work/partner/** --delivery copy";
    const ran = bridge.ran();
    expect(ran.indexOf(`${argv} --dry-run`)).toBeGreaterThanOrEqual(0);
    expect(ran.indexOf(`${argv} --dry-run`)).toBeLessThan(ran.indexOf(argv));
  });

  it("context.client-edit: a delivery change shows the plan, then the next list and the composed text carry it", async () => {
    const user = await openLayers();
    expect(layerNamed("client-acme")).toMatchObject({ delivery: "copy" });
    await user.click(screen.getByRole("button", { name: "Edit" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Edit layer client-acme" }),
    );
    await user.click(
      form.getByRole("checkbox", { name: /Keep the imported text inside the layer/ }),
    );
    await user.click(form.getByRole("button", { name: "Review the layer" }));
    const box = await review(/Change layer client-acme\?/);
    expect(box.getByText("Edit the client layer client-acme")).toBeVisible();
    expect(layerNamed("client-acme")).toMatchObject({ delivery: "copy" });
    await confirmResult(user, /Change layer client-acme\?/, "Save changes");
    await closeResult(user);
    expect(layerNamed("client-acme")).toMatchObject({ delivery: "import" });
    const argv = "context client edit client-acme --delivery import";
    const ran = bridge.ran();
    expect(ran.indexOf(`${argv} --dry-run`)).toBeGreaterThanOrEqual(0);
    expect(ran.indexOf(`${argv} --dry-run`)).toBeLessThan(ran.indexOf(argv));
    await waitFor(() =>
      expect(
        within(screen.getByRole("region", { name: "Layer client-acme" })).getByText(
          "an @import line",
        ),
      ).toBeVisible(),
    );
  });

  it("context.client-rm: deleting a client layer needs its name typed and drops it from the next list", async () => {
    const user = await openLayers();
    await user.click(screen.getByRole("button", { name: "Delete…" }));
    const form = within(
      await screen.findByRole("dialog", { name: "Delete layer client-acme" }),
    );
    await user.click(form.getByRole("button", { name: "Review the deletion" }));
    const box = await review(/Delete layer client-acme\?/);
    expect(box.getByText("Remove the client layer client-acme")).toBeVisible();
    expect(layerNamed("client-acme")).toBeDefined();
    await confirmResult(
      user,
      /Delete layer client-acme\?/,
      "Delete layer",
      "client-acme",
    );
    await closeResult(user);
    expect(layerNamed("client-acme")).toBeUndefined();
    expect(bridge.ran()).toContain("context client rm client-acme");
    await waitFor(() =>
      expect(layers().queryByRole("button", { name: /client-acme/ })).toBeNull(),
    );
  });
});
