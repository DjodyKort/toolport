import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { ContextScreen } from "./ContextScreen";
import { STATUSLINE, SHIMS, bareProfile, emptyStatus } from "./fixtures";
import { createBridge, failure, goldenData, wire, type Bridge } from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  bridge = createBridge();
  wire({ invoke, listen }, bridge);
});

async function open() {
  const user = userEvent.setup();
  render(<ContextScreen onOpenCommands={() => {}} />);
  await screen.findByRole("list", { name: "Launch profiles" });
  await screen.findByRole("list", { name: "Tokens per layer" });
  await screen.findByRole("list", { name: "Profile per folder" });
  return user;
}

const dialog = (name: RegExp) => screen.findByRole("dialog", { name });
const section = (name: string) => screen.getByRole("region", { name });

describe("Context screen: the tabs", () => {
  it("shows the tabs of the mockup, builds Launch & shell and marks the rest", async () => {
    const user = await open();
    const tabs = screen.getByRole("tablist", { name: "Context sections" });
    expect(
      within(tabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["This folder", "Profiles", "Layers", "Hooks", "Launch & shell"]);
    await user.click(within(tabs).getByRole("tab", { name: "Profiles" }));
    expect(screen.getByText(/built by MIG-GUI-10\b/)).toBeInTheDocument();
    await user.click(within(tabs).getByRole("tab", { name: "Hooks" }));
    expect(screen.getByText(/built by MIG-GUI-12\b/)).toBeInTheDocument();
  });
});

describe("Launch & shell: reading", () => {
  it("shows the launch profiles, the layers and the shim file of the fixture home", async () => {
    await open();
    const profiles = within(screen.getByRole("list", { name: "Launch profiles" }));
    expect(profiles.getByText("bare")).toBeInTheDocument();
    expect(profiles.getByText("claude-bare")).toBeInTheDocument();
    expect(profiles.getByText("Generated")).toBeInTheDocument();
    expect(profiles.getByText(/No org file/)).toBeInTheDocument();
    const layers = within(screen.getByRole("list", { name: "Layers" }));
    expect(layers.getByText("always on")).toBeInTheDocument();
    expect(layers.getByText("**/clients/acme/**")).toBeInTheDocument();
    const shell = within(section("Shell shims"));
    expect(await shell.findByText(SHIMS)).toBeInTheDocument();
    expect(shell.getByText("Written")).toBeInTheDocument();
  });

  it("says what each function in the shims file does and runs the source-order check", async () => {
    await open();
    const shell = within(section("Shell shims"));
    const functions = within(await shell.findByRole("list", { name: "Functions" }));
    expect(functions.getByText("mcpm_context_presync")).toBeInTheDocument();
    expect(
      functions.getByText(/Starts Claude in the launch profile bare/),
    ).toBeInTheDocument();
    expect(await shell.findByText("Needs a look")).toBeInTheDocument();
    expect(
      within(shell.getByRole("list", { name: "Shell checks" })).getByText(
        /sourced BEFORE shell-wrapper\.sh/,
      ),
    ).toBeInTheDocument();
    expect(shell.getByText(/still reads 1 line from the old folder/)).toBeInTheDocument();
    expect(shell.getByText("toolup")).toBeInTheDocument();
  });

  it("shows what loads with the token cost per layer, from the real golden", async () => {
    await open();
    const layers = within(screen.getByRole("list", { name: "Tokens per layer" }));
    const org = layers.getByText(/Org file/).closest("li")!;
    expect(within(org).getByText("10,075")).toBeInTheDocument();
    expect(layers.getByText(/Personal layer/)).toBeInTheDocument();
    expect(screen.getByText(/13,862/)).toBeInTheDocument();
    expect(screen.getByText(/5,666 more load on demand/)).toBeInTheDocument();
    expect(
      screen.getByText(/Skill list: 1,998 of 2,000 tokens; 17 skills/),
    ).toBeInTheDocument();
    const meter = within(org).getByRole("meter", { name: "Org file tokens" });
    expect(meter).toHaveAttribute("aria-valuenow", "10075");
  });

  it("lists the folders with the profile that applies and reads without writing or --home", async () => {
    await open();
    const folders = within(screen.getByRole("list", { name: "Profile per folder" }));
    expect(folders.getByText(/no mapping matches this folder/)).toBeInTheDocument();
    expect(within(section("Folder routing")).getByText("Off")).toBeInTheDocument();
    expect(bridge.ran().filter((line) => line.includes("--dry-run"))).toEqual([]);
    expect(bridge.ran().some((line) => line.includes("--home"))).toBe(false);
    expect(bridge.ran().filter((line) => line !== "commands")).toEqual(
      expect.arrayContaining([
        "context status",
        "context plan",
        "context profile list",
        "context client list",
        "context loads",
        "context folders",
      ]),
    );
  });
});

describe("Launch & shell: states", () => {
  it("shows an error with Retry for a read that fails and recovers", async () => {
    bridge.set("context status", failure("io", "cannot read the home"));
    const user = userEvent.setup();
    render(<ContextScreen onOpenCommands={() => {}} />);
    const shell = within(await screen.findByRole("region", { name: "Shell shims" }));
    expect(await shell.findByRole("alert")).toHaveTextContent("cannot read the home");
    bridge.set("context status", goldenData("context-status"));
    await user.click(shell.getByRole("button", { name: "Retry" }));
    expect(await shell.findByText("Not written")).toBeInTheDocument();
  });

  it("shows the empty states with the action that fills them", async () => {
    bridge.set("context status", emptyStatus);
    bridge.set("context profile list", { profiles: [] });
    bridge.set("context client list", { layers: [] });
    render(<ContextScreen onOpenCommands={() => {}} />);
    expect(await screen.findByText(/No launch profiles\./)).toBeInTheDocument();
    expect(await screen.findByText(/No layers yet/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Set up…" }).length).toBeGreaterThan(0);
  });

  it("shows every section as failed, with Retry, when the bridge cannot run toolportctl", async () => {
    invoke.mockReset().mockImplementation(async (command: string) => {
      if (command === "plus_ctl") throw new Error("toolportctl was not found");
      throw new Error(`unexpected invoke ${command}`);
    });
    render(<ContextScreen onOpenCommands={() => {}} />);
    await waitFor(() =>
      expect(screen.getAllByRole("alert").length).toBeGreaterThanOrEqual(6),
    );
    expect(screen.getAllByText(/toolportctl was not found/).length).toBeGreaterThan(0);
    expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
  });
});

describe("Launch & shell: deploy", () => {
  it("previews with the files context plan lists, then syncs", async () => {
    const user = await open();
    const deploy = within(section("Deploy"));
    const planned = within(await deploy.findByRole("list", { name: "Changes" }));
    expect(planned.getByText(SHIMS)).toBeInTheDocument();
    const before = planned.getAllByRole("listitem").map((li) => li.textContent);
    await user.click(deploy.getByRole("button", { name: "Sync…" }));
    const review = within(await dialog(/Sync the context files\?/));
    const files = within(review.getByRole("list", { name: "Changes" }))
      .getAllByRole("listitem")
      .map((li) => li.textContent);
    expect(files).toEqual(before);
    expect(bridge.ran()).toContain("context sync --dry-run");
    expect(bridge.ran()).not.toContain("context sync");
    await user.click(review.getByRole("button", { name: "Sync" }));
    const done = within(await dialog(/Sync the context files/));
    expect(await done.findByText("Deployed")).toBeInTheDocument();
    expect(done.getByText("Save the context config (1 profile(s))")).toBeInTheDocument();
    expect(bridge.ran().filter((line) => line === "context sync")).toHaveLength(1);
  });

  it("passes the options to the plan and to the apply, and shows the diff of the shell file", async () => {
    const user = await open();
    const deploy = within(section("Deploy"));
    await user.click(
      deploy.getByRole("checkbox", { name: /Point the shell at Toolport/ }),
    );
    await user.click(deploy.getByRole("checkbox", { name: /Do not save the config/ }));
    await waitFor(() => expect(bridge.ran()).toContain("context plan --rewrite-zshrc"));
    await user.click(deploy.getByRole("button", { name: "Apply…" }));
    const review = within(await dialog(/Apply the context files\?/));
    expect(await review.findByText(/Line 3 of the shell rc file/)).toBeInTheDocument();
    expect(review.getByText(/sourced before shell-wrapper\.sh/)).toBeInTheDocument();
    expect(bridge.ran()).toContain(
      "context apply --rewrite-zshrc --no-persist --dry-run",
    );
    await user.click(review.getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(bridge.ran()).toContain("context apply --rewrite-zshrc --no-persist"),
    );
  });

  it("moves the old shell lines with a previewed sync --rewrite-zshrc", async () => {
    const user = await open();
    const shell = within(section("Shell shims"));
    await user.click(await shell.findByRole("button", { name: "Move…" }));
    const review = within(await dialog(/Move the shell lines/));
    await review.findByRole("list", { name: "Changes" });
    expect(bridge.ran()).toContain("context sync --rewrite-zshrc --dry-run");
  });
});

describe("Launch & shell: launch profiles", () => {
  it("adds a profile through the preview, then reads the list again", async () => {
    const user = await open();
    const argv = "context profile add work --no-org --rules none --servers none";
    bridge.set(`${argv} --dry-run`, goldenData("context-profile-add.preview"));
    bridge.set(argv, () => {
      bridge.set("context profile list", {
        profiles: [
          bareProfile,
          { ...goldenData("context-profile-add.apply").profile, name: "work" },
        ],
      });
      return goldenData("context-profile-add.apply");
    });
    await user.click(await screen.findByRole("button", { name: "Add profile…" }));
    const form = within(await dialog(/Add a launch profile/));
    expect(form.getByRole("button", { name: "Preview" })).toBeDisabled();
    await user.type(form.getByLabelText("Name"), "work");
    await user.click(form.getByRole("checkbox", { name: "Include the org file" }));
    await user.clear(form.getByLabelText("Rules"));
    await user.type(form.getByLabelText("Rules"), "none");
    await user.clear(form.getByLabelText("Servers"));
    await user.type(form.getByLabelText("Servers"), "none");
    await user.click(form.getByRole("button", { name: "Preview" }));
    const review = within(await dialog(/Add launch profile work\?/));
    expect(
      await review.findByText(/Launch profile work \(0 server\(s\)\)/),
    ).toBeInTheDocument();
    expect(bridge.ran()).not.toContain(argv);
    await user.click(review.getByRole("button", { name: "Add profile" }));
    await waitFor(() => expect(bridge.ran()).toContain(argv));
    await user.click((await screen.findAllByRole("button", { name: "Close" }))[0]);
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Launch profiles" })).getAllByRole(
          "listitem",
        ),
      ).toHaveLength(2),
    );
  });

  it("removes a profile and its folder only after its name is typed", async () => {
    const user = await open();
    bridge.set(
      "context profile remove bare --purge --dry-run",
      goldenData("context-profile-remove.preview"),
    );
    bridge.set(
      "context profile remove bare --purge",
      goldenData("context-profile-remove.apply"),
    );
    await user.click(await screen.findByRole("button", { name: "Remove bare" }));
    const form = within(await dialog(/Remove launch profile bare/));
    await user.click(form.getByRole("checkbox", { name: /Also delete its folder/ }));
    await user.click(form.getByRole("button", { name: "Preview" }));
    const review = within(await dialog(/Remove launch profile bare\?/));
    expect(await review.findByText(/Launch profile folder/)).toBeInTheDocument();
    const confirm = review.getByRole("button", { name: "Remove profile" });
    expect(confirm).toBeDisabled();
    await user.type(review.getByLabelText(/Type bare to confirm/), "bare");
    expect(confirm).toBeEnabled();
    expect(bridge.ran()).not.toContain("context profile remove bare --purge");
    await user.click(confirm);
    await waitFor(() =>
      expect(bridge.ran()).toContain("context profile remove bare --purge"),
    );
  });

  it("disables the shims and purges the profile folders after a typed confirmation", async () => {
    const user = await open();
    bridge.set(
      "context disable --purge-profiles --dry-run",
      goldenData("context-disable.preview"),
    );
    bridge.set("context disable --purge-profiles", goldenData("context-disable.apply"));
    await user.click(await screen.findByRole("button", { name: "Disable shims…" }));
    const form = within(await dialog(/Disable the shell shims/));
    await user.click(
      form.getByRole("checkbox", { name: /Also delete the launch profile folders/ }),
    );
    await user.click(form.getByRole("button", { name: "Preview" }));
    const review = within(await dialog(/Disable the shell shims\?/));
    await review.findByText("Shell shims file");
    const confirm = review.getByRole("button", { name: "Disable" });
    expect(confirm).toBeDisabled();
    await user.type(review.getByLabelText(/Type disable to confirm/), "disable");
    await user.click(confirm);
    await waitFor(() =>
      expect(bridge.ran()).toContain("context disable --purge-profiles"),
    );
  });
});

describe("Launch & shell: layers, init and folder routing", () => {
  it("adds a client layer through the preview", async () => {
    const user = await open();
    bridge.set(
      "context client add acme --dry-run",
      goldenData("context-client-add.preview"),
    );
    bridge.set("context client add acme", goldenData("context-client-add.apply"));
    await user.click(await screen.findByRole("button", { name: "Add client layer…" }));
    const form = within(await dialog(/Add a client layer/));
    await user.type(form.getByLabelText("Name"), "acme");
    await user.click(form.getByRole("button", { name: "Preview" }));
    const review = within(await dialog(/Add client layer acme\?/));
    expect(
      await review.findByText(/Layer client-acme for \*\*\/clients\/acme\/\*\*/),
    ).toBeInTheDocument();
    await user.click(review.getByRole("button", { name: "Add layer" }));
    await waitFor(() => expect(bridge.ran()).toContain("context client add acme"));
  });

  it("runs the init wizard as a preview of the personal layer and the config", async () => {
    bridge.set("context status", emptyStatus);
    bridge.set("context init --yes --dry-run", goldenData("context-init.preview"));
    bridge.set("context init --yes", goldenData("context-init.apply"));
    const user = userEvent.setup();
    render(<ContextScreen onOpenCommands={() => {}} />);
    const layers = within(
      await screen.findByRole("region", { name: "Personal and client layers" }),
    );
    await user.click(await layers.findByRole("button", { name: "Set up…" }));
    const form = within(await dialog(/Set up the personal layer/));
    await user.click(form.getByRole("button", { name: "Preview" }));
    const review = within(await dialog(/Set up the personal layer\?/));
    expect(await review.findByText("Personal layer")).toBeInTheDocument();
    expect(review.getByText("Context config")).toBeInTheDocument();
    await user.click(review.getByRole("button", { name: "Set up" }));
    await waitFor(() => expect(bridge.ran()).toContain("context init --yes"));
  });

  it("turns folder routing on with a confirmation, because that command has no preview", async () => {
    const user = await open();
    bridge.set("context folders --enable", {
      ...goldenData("context-folders"),
      enabled: true,
    });
    await user.click(
      within(section("Folder routing")).getByRole("button", { name: "Turn on…" }),
    );
    const review = within(await dialog(/Turn folder routing on\?/));
    expect(review.getByText(/no preview/)).toBeInTheDocument();
    expect(review.getByText("toolportctl context folders --enable")).toBeInTheDocument();
    expect(bridge.ran()).not.toContain("context folders --enable --dry-run");
    expect(bridge.ran()).not.toContain("context folders --enable");
    await user.click(review.getByRole("button", { name: "Turn on" }));
    await waitFor(() => expect(bridge.ran()).toContain("context folders --enable"));
  });
});

describe("Launch & shell: checkpoint gauge", () => {
  async function check(user: ReturnType<typeof userEvent.setup>) {
    const checkpoint = within(section("Checkpoint"));
    await user.click(checkpoint.getByLabelText("Statusline JSON"));
    await user.paste(STATUSLINE);
    await user.type(checkpoint.getByLabelText(/Checkpoint at/), "50000");
    await user.click(checkpoint.getByRole("button", { name: "Check" }));
    return checkpoint;
  }

  it("sends the statusline JSON on stdin only and shows the gauge", async () => {
    const user = await open();
    const checkpoint = await check(user);
    const meter = await checkpoint.findByRole("meter", { name: "Context used" });
    expect(meter).toHaveAttribute("aria-valuenow", "1500");
    expect(
      checkpoint.getByText(/148,500 to the checkpoint at 150,000/),
    ).toBeInTheDocument();
    expect(bridge.stdins().map((call) => call.stdin)).toEqual([STATUSLINE]);
    expect(bridge.stdins()[0].argv).toEqual([
      "context",
      "checkpoint-status",
      "--checkpoint-at",
      "50000",
    ]);
    expect(bridge.ran().join("\n")).not.toContain("model-x");
  });

  it("never shows the pasted JSON back, not even in a failure", async () => {
    bridge.set(
      "context checkpoint-status --checkpoint-at 50000",
      failure(
        "bad_input",
        "statusline JSON: EOF while parsing a value at line 1 column 0",
      ),
    );
    const user = await open();
    const checkpoint = await check(user);
    expect(await checkpoint.findByRole("alert")).toHaveTextContent(/EOF while parsing/);
    expect(checkpoint.getByRole("alert").textContent).not.toContain("model-x");
    expect(
      (checkpoint.getByLabelText("Statusline JSON") as HTMLTextAreaElement).value,
    ).toBe(STATUSLINE);
  });

  it("asks for numbers only and keeps Check off until there is JSON", async () => {
    const user = await open();
    const checkpoint = within(section("Checkpoint"));
    expect(checkpoint.getByRole("button", { name: "Check" })).toBeDisabled();
    await user.click(checkpoint.getByLabelText("Statusline JSON"));
    await user.paste("{}");
    await user.type(checkpoint.getByLabelText(/Window/), "12k");
    expect(checkpoint.getByRole("button", { name: "Check" })).toBeDisabled();
  });
});
