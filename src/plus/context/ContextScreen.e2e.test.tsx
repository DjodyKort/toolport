import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { finish, mountContext, openContext, review, section, visibleText } from "./e2e";
import { SHIMS } from "./fixtures";
import { createBridge, failure, wire, type Bridge } from "./testkit";

/** The Context screen's Launch & shell tab walked the way a person uses it, against a world
 * that changes: a preview never changes it, an apply does, and the next read shows it. Each
 * test is named by the parity action it proves (`src/plus/gui-parity.json`). */
let bridge: Bridge;
const start = (fresh = false) => {
  bridge = createBridge({ world: fresh ? { fresh: true } : true });
  wire({ invoke, listen }, bridge);
};
const world = () => bridge.world!.snapshot();
const ran = (line: string) => bridge.ran().filter((one) => one === line).length;

beforeEach(() => start());

afterEach(() => {
  expect(bridge.missing).toEqual([]);
  const stray = bridge.ran().filter((line) => !/^(commands|context)( |$)/.test(line));
  expect(stray, "the screen only runs its own commands").toEqual([]);
  expect(bridge.ran().some((line) => /secret|--reveal|--home/.test(line))).toBe(false);
  expect(
    bridge.stdins().map((call) => call.argv.slice(0, 2).join(" ")),
    "only the checkpoint reads stdin",
  ).toEqual(bridge.stdins().map(() => "context checkpoint-status"));
});

const list = (name: string) => within(screen.getByRole("list", { name }));
const items = (name: string) =>
  list(name)
    .getAllByRole("listitem")
    .map((li) => li.textContent);

async function escape(user: UserEvent) {
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

describe("Context screen, reading the fixture home", () => {
  it("context.status, context.profile-list, context.client-list: show the layers, the profile and the shims file", async () => {
    await openContext();
    expect(items("Launch profiles")).toEqual([expect.stringContaining("claude-bare")]);
    expect(items("Layers")).toEqual([
      expect.stringContaining("personal"),
      expect.stringContaining("client-acme"),
    ]);
    expect(list("Layers").getByText("**/clients/acme/**")).toBeVisible();
    const shell = within(section("Shell shims"));
    expect(shell.getByText(SHIMS)).toBeVisible();
    expect(shell.getByText("Written")).toBeVisible();
    expect(shell.getByText(/still reads 1 line from the old folder/)).toBeVisible();
    expect(bridge.ran()).toEqual(
      expect.arrayContaining([
        "context status",
        "context profile list",
        "context client list",
      ]),
    );
    expect(
      bridge.ran().filter((line) => /--dry-run|--enable|--disable/.test(line)),
    ).toEqual([]);
  });

  it("context.plan: lists the files, and the options change the plan that is read", async () => {
    const user = await openContext();
    const deploy = within(section("Deploy"));
    expect(
      within(await deploy.findByRole("list", { name: "Changes" })).getByText(SHIMS),
    ).toBeVisible();
    await user.click(
      deploy.getByRole("checkbox", { name: /Point the shell at Toolport/ }),
    );
    await waitFor(() => expect(bridge.ran()).toContain("context plan --rewrite-zshrc"));
    expect(await deploy.findByText(/Line 3 of the shell rc file/)).toBeVisible();
    await user.click(deploy.getByRole("checkbox", { name: /Deploy the rules too/ }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("context plan --rules --rewrite-zshrc"),
    );
    expect(world().shims).toBe(true);
    expect(bridge.ran().filter((line) => /^context (apply|sync)/.test(line))).toEqual([]);
  });
});

describe("Context screen, Deploy", () => {
  it("context.apply: the preview lists what the plan lists and writes nothing, Escape cancels, confirming writes the shims", async () => {
    start(true);
    const user = await openContext();
    const deploy = within(section("Deploy"));
    const planned = within(await deploy.findByRole("list", { name: "Changes" }));
    const before = planned.getAllByRole("listitem").map((li) => li.textContent);
    expect(within(section("Shell shims")).getByText("Not written")).toBeVisible();

    await user.click(deploy.getByRole("button", { name: "Apply…" }));
    const box = await review(/Apply the context files\?/);
    expect(
      within(box.getByRole("list", { name: "Changes" }))
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(before);
    expect(ran("context apply --dry-run")).toBe(1);
    expect(ran("context apply")).toBe(0);
    await escape(user);
    expect(ran("context apply")).toBe(0);
    expect(world().shims).toBe(false);

    await user.click(deploy.getByRole("button", { name: "Apply…" }));
    await finish(user, /Apply the context files\?/, "Apply", { done: "Deployed" });
    expect(ran("context apply")).toBe(1);
    expect(world()).toMatchObject({ shims: true, config: true });
    expect(await within(section("Shell shims")).findByText("Written")).toBeVisible();
  });

  it("context.apply: --no-persist writes the shims and leaves the config alone", async () => {
    start(true);
    const user = await openContext();
    const deploy = within(section("Deploy"));
    await user.click(deploy.getByRole("checkbox", { name: /Do not save the config/ }));
    await user.click(deploy.getByRole("button", { name: "Apply…" }));
    await review(/Apply the context files\?/);
    expect(ran("context apply --no-persist --dry-run")).toBe(1);
    await finish(user, /Apply the context files\?/, "Apply", { done: "Deployed" });
    expect(ran("context apply --no-persist")).toBe(1);
    expect(world()).toMatchObject({ shims: true, config: false });
  });

  it("context.sync: previews the same files, then syncs and says what was written", async () => {
    const user = await openContext();
    const deploy = within(section("Deploy"));
    const planned = within(await deploy.findByRole("list", { name: "Changes" }));
    const before = planned.getAllByRole("listitem").map((li) => li.textContent);
    await user.click(deploy.getByRole("button", { name: "Sync…" }));
    const box = await review(/Sync the context files\?/);
    expect(
      within(box.getByRole("list", { name: "Changes" }))
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(before);
    expect(ran("context sync")).toBe(0);
    await user.click(box.getByRole("button", { name: "Sync" }));
    expect(
      await screen.findByText("Save the context config (1 profile(s))"),
    ).toBeVisible();
    expect(ran("context sync")).toBe(1);
    expect(world().legacy).toHaveLength(1);
  });
});

describe("Context screen, Launch profiles", () => {
  it("context.profile-add: the form needs a name, the preview adds nothing, confirming adds the profile and its shell function", async () => {
    start(true);
    const user = await openContext();
    const profiles = within(section("Launch profiles"));
    expect(profiles.getByText(/No launch profiles\./)).toBeVisible();
    await user.click(profiles.getByRole("button", { name: "Add profile…" }));
    const form = within(
      await screen.findByRole("dialog", { name: /Add a launch profile/ }),
    );
    expect(form.getByRole("button", { name: "Preview" })).toBeDisabled();
    await user.type(form.getByLabelText("Name"), "work");
    await user.keyboard("{Enter}");
    const box = await review(/Add launch profile work\?/);
    expect(await box.findByText(/Launch profile work \(0 server\(s\)\)/)).toBeVisible();
    expect(ran("context profile add work --dry-run")).toBe(1);
    expect(ran("context profile add work")).toBe(0);
    await escape(user);
    expect(world().profiles).toEqual([]);

    await user.click(profiles.getByRole("button", { name: "Add profile…" }));
    const again = within(
      await screen.findByRole("dialog", { name: /Add a launch profile/ }),
    );
    await user.type(again.getByLabelText("Name"), "work");
    await user.click(again.getByRole("button", { name: "Preview" }));
    await finish(user, /Add launch profile work\?/, "Add profile", {
      done: "Launch profile defined",
    });
    expect(ran("context profile add work")).toBe(1);
    expect(world()).toMatchObject({ shims: true, config: true });
    expect(world().profiles.map((p) => p.name)).toEqual(["work"]);
    await waitFor(() =>
      expect(items("Launch profiles")).toEqual([expect.stringContaining("claude-work")]),
    );
    expect(list("Launch profiles").getByText("Generated")).toBeVisible();
    expect(
      await within(section("Deploy")).findByText("Launch profile work (0 server(s))"),
    ).toBeVisible();
    expect(
      within(
        await within(section("Shell shims")).findByRole("list", { name: "Functions" }),
      ).getByText("claude-work"),
    ).toBeVisible();
  });

  it("context.profile-remove: --purge asks for the name, Escape cancels, the right name removes the profile", async () => {
    const user = await openContext();
    await user.click(screen.getByRole("button", { name: "Remove bare" }));
    const form = within(
      await screen.findByRole("dialog", { name: /Remove launch profile bare/ }),
    );
    await user.click(form.getByRole("checkbox", { name: /Also delete its folder/ }));
    await user.click(form.getByRole("button", { name: "Preview" }));
    const box = await review(/Remove launch profile bare\?/);
    expect(await box.findByText(/Launch profile folder/)).toBeVisible();
    expect(ran("context profile remove bare --purge --dry-run")).toBe(1);
    await user.type(box.getByLabelText(/Type bare to confirm/), "bar");
    expect(box.getByRole("button", { name: "Remove profile" })).toBeDisabled();
    await escape(user);
    expect(ran("context profile remove bare --purge")).toBe(0);
    expect(world().profiles).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "Remove bare" }));
    const next = within(
      await screen.findByRole("dialog", { name: /Remove launch profile bare/ }),
    );
    await user.click(next.getByRole("checkbox", { name: /Also delete its folder/ }));
    await user.click(next.getByRole("button", { name: "Preview" }));
    await finish(user, /Remove launch profile bare\?/, "Remove profile", {
      typed: "bare",
      done: "Launch profile removed",
    });
    expect(ran("context profile remove bare --purge")).toBe(1);
    expect(world().profiles).toEqual([]);
    expect(
      await within(section("Launch profiles")).findByText(/No launch profiles\./),
    ).toBeVisible();
  });
});

describe("Context screen, Shell shims", () => {
  it("context.disable: --purge-profiles asks for a typed confirmation, then the shims file and the profile folders are gone", async () => {
    const user = await openContext();
    const shell = within(section("Shell shims"));
    await user.click(shell.getByRole("button", { name: "Disable shims…" }));
    const form = within(
      await screen.findByRole("dialog", { name: /Disable the shell shims/ }),
    );
    await user.click(
      form.getByRole("checkbox", { name: /Also delete the launch profile folders/ }),
    );
    await user.click(form.getByRole("button", { name: "Preview" }));
    const box = await review(/Disable the shell shims\?/);
    expect(await box.findByText("Shell shims file")).toBeVisible();
    expect(box.getByText("Launch profile folder")).toBeVisible();
    expect(box.getByRole("button", { name: "Disable" })).toBeDisabled();
    expect(ran("context disable --purge-profiles --dry-run")).toBe(1);
    await escape(user);
    expect(ran("context disable --purge-profiles")).toBe(0);
    expect(world().shims).toBe(true);

    await user.click(shell.getByRole("button", { name: "Disable shims…" }));
    const next = within(
      await screen.findByRole("dialog", { name: /Disable the shell shims/ }),
    );
    await user.click(
      next.getByRole("checkbox", { name: /Also delete the launch profile folders/ }),
    );
    await user.click(next.getByRole("button", { name: "Preview" }));
    await finish(user, /Disable the shell shims\?/, "Disable", {
      typed: "disable",
      done: "Shims removed",
    });
    expect(world()).toMatchObject({ shims: false });
    expect(world().profiles[0].generated).toBe(false);
    expect(await shell.findByText("Not written")).toBeVisible();
    expect(
      await within(section("Launch profiles")).findByText("Not generated"),
    ).toBeVisible();
    expect(shell.getByText(/shims file missing/)).toBeVisible();
  });

  it("context.status: moving the old shell lines is a previewed sync --rewrite-zshrc that edits the next read", async () => {
    const user = await openContext();
    const shell = within(section("Shell shims"));
    expect(shell.getByText("Needs a look")).toBeVisible();
    await user.click(shell.getByRole("button", { name: "Move…" }));
    const box = await review(/Move the shell lines/);
    expect(await box.findByText(/Line 3 of the shell rc file/)).toBeVisible();
    expect(ran("context sync --rewrite-zshrc --dry-run")).toBe(1);
    await escape(user);
    expect(ran("context sync --rewrite-zshrc")).toBe(0);
    expect(world().legacy).toHaveLength(1);

    await user.click(shell.getByRole("button", { name: "Move…" }));
    await finish(user, /Move the shell lines/, "Rewrite the shell file", {
      done: "Deployed",
    });
    expect(world().legacy).toEqual([]);
    await waitFor(() => expect(shell.queryByText(/still reads/)).toBeNull());
  });
});

describe("Context screen, Layers", () => {
  it("context.init: the wizard previews the personal layer and the config, and scaffolds them on confirm", async () => {
    start(true);
    const user = await openContext();
    const layers = within(section("Personal and client layers"));
    expect(layers.getByText(/No layers yet/)).toBeVisible();
    await user.click(layers.getAllByRole("button", { name: "Set up…" })[0]);
    const form = within(
      await screen.findByRole("dialog", { name: /Set up the personal layer/ }),
    );
    await user.click(form.getByRole("button", { name: "Preview" }));
    const box = await review(/Set up the personal layer\?/);
    expect(await box.findByText("Personal layer")).toBeVisible();
    expect(box.getByText("Context config")).toBeVisible();
    expect(ran("context init --yes --dry-run")).toBe(1);
    await escape(user);
    expect(ran("context init --yes")).toBe(0);
    expect(world().layers).toEqual([]);

    await user.click(layers.getAllByRole("button", { name: "Set up…" })[0]);
    const next = within(
      await screen.findByRole("dialog", { name: /Set up the personal layer/ }),
    );
    await user.click(next.getByRole("button", { name: "Preview" }));
    await finish(user, /Set up the personal layer\?/, "Set up", {
      done: "Personal layer set up",
    });
    expect(world()).toMatchObject({ config: true });
    await waitFor(() =>
      expect(items("Layers")).toEqual([expect.stringContaining("personal")]),
    );
    expect(list("Layers").getByText("always on")).toBeVisible();
    expect(layers.queryByRole("button", { name: "Set up…" })).toBeNull();
  });

  it("context.client-add: a folder pattern makes a client layer that shows up with its glob", async () => {
    const user = await openContext();
    const layers = within(section("Personal and client layers"));
    await user.click(layers.getByRole("button", { name: "Add client layer…" }));
    const form = within(
      await screen.findByRole("dialog", { name: /Add a client layer/ }),
    );
    expect(form.getByRole("button", { name: "Preview" })).toBeDisabled();
    await user.type(form.getByLabelText("Name"), "partner");
    await user.type(form.getByLabelText("Folder pattern"), "**/work/partner/**");
    await user.click(form.getByRole("button", { name: "Preview" }));
    const box = await review(/Add client layer partner\?/);
    expect(
      await box.findByText(/Layer client-partner for \*\*\/work\/partner\/\*\*/),
    ).toBeVisible();
    expect(ran("context client add partner --glob **/work/partner/** --dry-run")).toBe(1);
    await escape(user);
    expect(world().layers).toHaveLength(2);

    await user.click(layers.getByRole("button", { name: "Add client layer…" }));
    const next = within(
      await screen.findByRole("dialog", { name: /Add a client layer/ }),
    );
    await user.type(next.getByLabelText("Name"), "partner");
    await user.type(next.getByLabelText("Folder pattern"), "**/work/partner/**");
    await user.click(next.getByRole("button", { name: "Preview" }));
    await finish(user, /Add client layer partner\?/, "Add layer", {
      done: "Client layer added",
    });
    expect(ran("context client add partner --glob **/work/partner/**")).toBe(1);
    await waitFor(() => expect(items("Layers")).toHaveLength(3));
    expect(list("Layers").getByText("**/work/partner/**")).toBeVisible();
    expect(world().layers.map((layer) => layer.name)).toContain("client-partner");
  });
});

describe("Context screen, What loads", () => {
  it("context.loads: shows the cost per layer, per launch profile and per folder", async () => {
    const user = await openContext();
    const loads = within(section("What loads"));
    const rows = within(loads.getByRole("list", { name: "Tokens per layer" }));
    expect(
      within(rows.getByText(/Org file/).closest("li")!).getByText("10,070"),
    ).toBeVisible();
    expect(rows.getByText(/Personal layer/)).toBeVisible();
    expect(loads.getByText(/10,601/)).toBeVisible();
    expect(loads.getByText(/5,666 more load on demand/)).toBeVisible();

    await user.selectOptions(loads.getByLabelText("Launch profile"), "bare");
    await waitFor(() => expect(bridge.ran()).toContain("context loads --profile bare"));
    expect(await loads.findByText(/with the profile bare/)).toBeVisible();
    expect(loads.queryByText(/Org file/)).toBeNull();

    await user.selectOptions(loads.getByLabelText("Launch profile"), "");
    await user.type(loads.getByLabelText("Folder"), "/work/app");
    await user.click(loads.getByRole("button", { name: "Show" }));
    await waitFor(() => expect(bridge.ran()).toContain("context loads --cwd /work/app"));
    expect(await loads.findByText("/work/app")).toBeVisible();
  });

  it("context.loads: reads again after a write, so a new layer and a new client layer change the numbers", async () => {
    start(true);
    const user = await openContext();
    const loads = within(section("What loads"));
    expect(loads.getByText(/10,110/)).toBeVisible();
    expect(loads.getByText(/5,610 more load on demand/)).toBeVisible();
    expect(loads.queryByText(/Personal layer/)).toBeNull();

    const layers = within(section("Personal and client layers"));
    await user.click(layers.getAllByRole("button", { name: "Set up…" })[0]);
    await user.click(
      within(
        await screen.findByRole("dialog", { name: /Set up the personal layer/ }),
      ).getByRole("button", { name: "Preview" }),
    );
    await finish(user, /Set up the personal layer\?/, "Set up", {
      done: "Personal layer set up",
    });
    expect(await loads.findByText(/Personal layer/)).toBeVisible();
    expect(await loads.findByText(/10,601/)).toBeVisible();

    await user.click(layers.getByRole("button", { name: "Add client layer…" }));
    const form = within(
      await screen.findByRole("dialog", { name: /Add a client layer/ }),
    );
    await user.type(form.getByLabelText("Name"), "partner");
    await user.click(form.getByRole("button", { name: "Preview" }));
    await finish(user, /Add client layer partner\?/, "Add layer", {
      done: "Client layer added",
    });
    expect(await loads.findByText(/5,666 more load on demand/)).toBeVisible();
  });
});

describe("Context screen, Folder routing", () => {
  it("context.folders: has no preview, so it asks first, Escape cancels, and the switch shows on the next read", async () => {
    const user = await openContext();
    const folders = within(section("Folder routing"));
    expect(folders.getByText("Off")).toBeVisible();
    expect(items("Profile per folder")).toEqual([
      expect.stringContaining("no mapping matches this folder"),
    ]);

    await user.click(folders.getByRole("button", { name: "Turn on…" }));
    const box = await review(/Turn folder routing on\?/);
    expect(box.getByText(/no preview/)).toBeVisible();
    expect(box.getByText("toolportctl context folders --enable")).toBeVisible();
    await escape(user);
    expect(ran("context folders --enable")).toBe(0);
    expect(world().folders).toBe(false);

    await user.click(folders.getByRole("button", { name: "Turn on…" }));
    await finish(user, /Turn folder routing on\?/, "Turn on", {
      done: "Folder routing is on",
    });
    expect(ran("context folders --enable")).toBe(1);
    expect(bridge.ran()).not.toContain("context folders --enable --dry-run");
    expect(world().folders).toBe(true);
    expect(await folders.findByText("On")).toBeVisible();

    await user.click(folders.getByRole("button", { name: "Turn off…" }));
    await finish(user, /Turn folder routing off\?/, "Turn off", {
      done: "Folder routing is off",
    });
    expect(world().folders).toBe(false);
    expect(await folders.findByText("Off")).toBeVisible();
  });
});

describe("Context screen, Checkpoint", () => {
  const CANARY = "canary-session-7f3a91";
  const STATUSLINE = JSON.stringify({
    session_id: CANARY,
    model: { id: "model-x" },
    context_window: { context_window_size: 200000, used_percentage: 0.75 },
  });

  async function check(user: UserEvent, json: string) {
    const checkpoint = within(section("Checkpoint"));
    await user.click(checkpoint.getByLabelText("Statusline JSON"));
    await user.paste(json);
    await user.type(checkpoint.getByLabelText(/Checkpoint at/), "50000");
    await user.click(checkpoint.getByRole("button", { name: "Check" }));
    return checkpoint;
  }

  it("context.checkpoint-status: shows the gauge for the statusline JSON that went to stdin", async () => {
    const user = await openContext();
    const checkpoint = await check(user, STATUSLINE);
    const meter = await checkpoint.findByRole("meter", { name: "Context used" });
    expect(meter).toHaveAttribute("aria-valuenow", "1500");
    expect(checkpoint.getByText(/148,500 to the checkpoint at 150,000/)).toBeVisible();
    expect(bridge.stdins().map((call) => call.stdin)).toEqual([STATUSLINE]);
    expect(bridge.stdins()[0].argv).toEqual([
      "context",
      "checkpoint-status",
      "--checkpoint-at",
      "50000",
    ]);
  });

  it("context.checkpoint-status: the statusline JSON is in no argv and in no text on the screen, not even after a failure", async () => {
    const user = await openContext();
    const checkpoint = await check(user, STATUSLINE);
    await checkpoint.findByRole("meter", { name: "Context used" });
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
    expect(visibleText()).not.toContain(CANARY);
    expect(visibleText()).not.toContain("model-x");

    bridge.set(
      "context checkpoint-status --checkpoint-at 50000",
      failure("bad_input", "statusline JSON: expected value at line 1 column 1"),
    );
    await user.click(checkpoint.getByRole("button", { name: "Check" }));
    expect(await checkpoint.findByRole("alert")).toHaveTextContent(/expected value/);
    expect(visibleText()).not.toContain(CANARY);
    for (const storage of [window.localStorage, window.sessionStorage]) {
      expect(JSON.stringify({ ...storage })).not.toContain(CANARY);
    }
    expect(bridge.stdins().map((call) => call.stdin)).toEqual([STATUSLINE, STATUSLINE]);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
  });

  it("context.checkpoint-status: a broken statusline is an error with the CLI's words and no echo of the input", async () => {
    const user = await openContext();
    const checkpoint = await check(user, `{"session_id": "${CANARY}", `);
    expect(await checkpoint.findByRole("alert")).toHaveTextContent(/statusline JSON/);
    expect(visibleText()).not.toContain(CANARY);
    expect(bridge.ran().join("\n")).not.toContain(CANARY);
  });
});

describe("Context screen, offline", () => {
  it("context.status: shows every section as failed while toolportctl is down, and writes work after Retry", async () => {
    let down = true;
    invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (down && command === "plus_ctl")
        throw new Error("toolportctl could not be started");
      return bridge.invoke(command, args);
    });
    const user = mountContext();
    const deploy = within(await screen.findByRole("region", { name: "Deploy" }));
    expect(await deploy.findByRole("alert")).toHaveTextContent(
      /toolportctl could not be started/,
    );
    await waitFor(() =>
      expect(screen.getAllByRole("alert").length).toBeGreaterThanOrEqual(6),
    );
    expect(screen.queryByRole("list", { name: "Launch profiles" })).toBeNull();

    down = false;
    await user.click(deploy.getByRole("button", { name: "Retry" }));
    expect(await deploy.findByRole("list", { name: "Changes" })).toBeVisible();
    await user.click(deploy.getByRole("button", { name: "Sync…" }));
    const box = await review(/Sync the context files\?/);
    expect(await box.findByRole("list", { name: "Changes" })).toBeVisible();
    expect(screen.queryByText(/has not loaded yet/)).toBeNull();
    expect(world().shims).toBe(true);
    expect(ran("context sync")).toBe(0);
  });
});
