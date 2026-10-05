import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }));

import { useFolderChoice } from "../context/folder";
import { HooksTab } from "./HooksTab";
import {
  createHooksBridge,
  FOLDER,
  golden,
  lsArgv,
  QUIET,
  SENTINEL,
  wire,
  type Bridge,
} from "./testkit";

let bridge: Bridge;
beforeEach(() => {
  window.localStorage.clear();
  bridge = createHooksBridge();
  wire({ invoke, listen }, bridge);
});

function Host({ onOpenPlugins }: { onOpenPlugins?: () => void }) {
  const here = useFolderChoice();
  return <HooksTab here={here} onOpenPlugins={onOpenPlugins} />;
}

async function open(folder = FOLDER, onOpenPlugins?: () => void) {
  const user = userEvent.setup();
  render(<Host onOpenPlugins={onOpenPlugins} />);
  await screen.findByRole("group", { name: "Hook counts" });
  if (folder) {
    await user.type(screen.getByLabelText("Folder"), folder);
    await user.click(screen.getByRole("button", { name: "Show" }));
    await waitFor(() => expect(bridge.count(lsArgv(folder))).toBeGreaterThan(0));
    await screen.findByRole("group", { name: "Hook counts" });
  }
  return user;
}

const list = (name: string) => screen.getByRole("list", { name });

describe("Context > Hooks", () => {
  it("lists what runs before and after a Bash call, with owner and switch method", async () => {
    await open();
    const before = within(list("Before Bash runs list")).getAllByRole("listitem");
    const after = within(list("After Bash ran list")).getAllByRole("listitem");
    expect(before).toHaveLength(8);
    expect(after).toHaveLength(4);
    expect(within(before[0]).getByText("user: settings.json")).toBeInTheDocument();
    expect(within(before[0]).getByText("edit settings.json")).toBeInTheDocument();
    const dispatcher = before.find((row) =>
      /pre:bash:dispatcher/.test(row.textContent ?? ""),
    );
    expect(dispatcher).toBeTruthy();
    expect(within(dispatcher!).getByText("plugin: ecc@ecc")).toBeInTheDocument();
    expect(within(dispatcher!).getByText("turn the plugin off")).toBeInTheDocument();
    expect(screen.getAllByText("background").length).toBeGreaterThan(0);
    expect(screen.getAllByText("waits").length).toBeGreaterThan(0);
  });

  it("counts processes and says they are counted from matchers, not timed", async () => {
    await open();
    const strip = screen.getByRole("group", { name: "Hook counts" });
    expect(within(strip).getByText("Processes for one Bash call")).toBeInTheDocument();
    expect(within(strip).getByText("12")).toBeInTheDocument();
    expect(within(strip).getByText("Run order")).toBeInTheDocument();
    expect(within(strip).getByText(/no per-hook switch/)).toBeInTheDocument();
    expect(screen.getByText(/counted from the matchers, not timed/)).toBeInTheDocument();
  });

  it("switches the tool without reading the hooks again", async () => {
    const user = await open();
    const reads = bridge.count(lsArgv(FOLDER));
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByRole("button", { name: "Edit" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Bash" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByRole("region", { name: "Before Edit runs" })).toBeInTheDocument();
    expect(bridge.count(lsArgv(FOLDER))).toBe(reads);
  });

  it("is operable from the keyboard", async () => {
    const user = await open();
    screen.getByRole("button", { name: "Write" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("button", { name: "Write" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("calls out hooks of different owners on the same tool", async () => {
    await open();
    expect(
      screen.getByText(/Hooks of different owners watch the same Bash call/),
    ).toBeInTheDocument();
    expect(screen.getByText(/strictest answer wins/)).toBeInTheDocument();
  });

  it("warns that a switched-off hook still starts a process", async () => {
    await open();
    expect(screen.getByText(/still starts a process/)).toBeInTheDocument();
  });

  it("links to the plugins screen", async () => {
    const onOpenPlugins = vi.fn();
    const user = await open(FOLDER, onOpenPlugins);
    await user.click(screen.getByRole("button", { name: "Plugins" }));
    expect(onOpenPlugins).toHaveBeenCalledTimes(1);
  });

  it("tells when disableAllHooks is set for the folder", async () => {
    await open(QUIET);
    expect(
      screen.getByText(/disableAllHooks is set for this folder/),
    ).toBeInTheDocument();
  });

  it("counts the hooks that run besides tool calls, per owner", async () => {
    await open();
    const strip = screen.getByRole("group", { name: "Hook counts" });
    expect(within(strip).getByText("32 more")).toBeInTheDocument();
    expect(within(strip).getByText(/skill: handoff-notes 1/)).toBeInTheDocument();
  });

  it("never shows an env value that rides along in a hook entry", async () => {
    const data = golden("hooks-ls.full");
    bridge.set(lsArgv(FOLDER), () => ({
      ...data,
      hooks: data.hooks.map((hook) => ({ ...hook, env: { TOKEN: SENTINEL } })),
    }));
    await open();
    expect(document.body.textContent).not.toContain(SENTINEL);
    expect(bridge.ran().join("\n")).not.toContain(SENTINEL);
  });
});

describe("Context > Hooks: states", () => {
  it("shows a loading state", async () => {
    bridge.set(lsArgv(), () => new Promise(() => {}));
    render(<Host />);
    expect(await screen.findByRole("status", { name: /Loading/ })).toBeInTheDocument();
  });

  it("shows the error with Retry", async () => {
    const { Failure } = await import("../skills/world");
    bridge.set(lsArgv(), new Failure("bridge", "the settings file could not be read"));
    const user = userEvent.setup();
    render(<Host />);
    expect(
      await screen.findByText(/settings file could not be read/),
    ).toBeInTheDocument();
    bridge.set(lsArgv(), () => golden("hooks-ls.full"));
    await user.click(screen.getByRole("button", { name: /Retry/ }));
    await screen.findByRole("group", { name: "Hook counts" });
  });

  it("says so when no hook is defined", async () => {
    const data = golden("hooks-ls.disabled");
    bridge.set(lsArgv(), () => ({ ...data, disabledAll: false, warnings: [] }));
    render(<Host />);
    expect(await screen.findByText("No hooks")).toBeInTheDocument();
  });
});
