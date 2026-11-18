import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { toast } from "sonner";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import LaunchProfiles from "./LaunchProfiles";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  const { invokeLog } = await import("../servers/testkit");
  return {
    invoke: async (command: string, args: Record<string, unknown>) => {
      invokeLog.push({ command, args });
      return plusFixtureIpc(command, args);
    },
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn(), message: vi.fn() }),
}));

const HOME = "/Users/example";
const FOLDER = `${HOME}/work/proj`;

function setup(
  folder: string | null = null,
  launchProfiles: Parameters<typeof resetPlusFixture>[0] = {},
) {
  resetPlusFixture({
    accessSets: ["local", "work"],
    launchProfiles: {
      profiles: [{ name: "bare", set: "work" }],
      ...launchProfiles.launchProfiles,
    },
  });
  return render(<LaunchProfiles home={HOME} folder={folder} />);
}

const ran = (...words: string[]) =>
  ranArgv().some((argv) => words.every((word) => argv.includes(word)));

beforeEach(() => {
  invokeLog.length = 0;
  vi.clearAllMocks();
});

describe("Launch profiles", () => {
  it("test00_the_parity_action_is_built_and_names_this_file", () => {
    expect(guiParity.actions["context.launch-profiles"]).toMatchObject({
      status: "built",
      test: "src/plus/context/LaunchProfiles.test.tsx",
    });
  });

  it("test01_a_profile_shows_its_client_id_and_access_set", async () => {
    setup();
    const row = (await screen.findByText("bare")).closest("div.flex-col") as HTMLElement;
    expect(row).toBeTruthy();
    expect(await screen.findByText("claude-code-bare")).toBeTruthy();
    expect(screen.getByText("generated")).toBeTruthy();
  });

  it("test02_adding_a_profile_shows_the_plan_before_it_writes", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("bare");
    await user.click(screen.getByRole("button", { name: "Add profile…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByLabelText("Name"), "lean");
    await user.selectOptions(within(form).getByLabelText("Access set"), "local");
    await user.click(within(form).getByRole("button", { name: "Preview the plan" }));
    await screen.findByText(/Claude Code started with claude-code-lean/);
    expect(ran("launch-profile", "add", "lean", "--dry-run")).toBe(true);
    expect(ranArgv().some((a) => a.includes("add") && !a.includes("--dry-run"))).toBe(
      false,
    );
    await user.click(screen.getByRole("button", { name: "Add profile" }));
    await waitFor(() =>
      expect(
        ranArgv().some(
          (a) =>
            a.includes("launch-profile") &&
            a.includes("add") &&
            a.includes("lean") &&
            !a.includes("--dry-run"),
        ),
      ).toBe(true),
    );
    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalled());
    const [message, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
    expect(message).toBe("Added lean");
    (options as unknown as { action: { onClick: () => void } }).action.onClick();
    await waitFor(() => expect(ran("launch-profile", "rm", "lean")).toBe(true));
  });

  it("test03_a_name_outside_the_shell_safe_set_cannot_be_previewed", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("bare");
    await user.click(screen.getByRole("button", { name: "Add profile…" }));
    const form = await screen.findByRole("dialog");
    await user.type(within(form).getByLabelText("Name"), "Bad Name");
    await user.selectOptions(within(form).getByLabelText("Access set"), "local");
    expect(within(form).getByRole("alert").textContent).toMatch(/Lower case letters/);
    expect(
      (
        within(form).getByRole("button", {
          name: "Preview the plan",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("test04_a_folder_mapping_that_outranks_the_profile_is_named_with_its_folder", async () => {
    setup(FOLDER, { launchProfiles: { folders: [{ path: FOLDER, set: "local" }] } });
    const note = await screen.findByRole("note");
    expect(note.textContent).toContain(FOLDER);
    expect(note.textContent).toContain(
      "A folder mapping outranks a client's own access set",
    );
  });

  it("test05_removing_shows_the_plan_and_offers_undo", async () => {
    const user = userEvent.setup();
    setup();
    await screen.findByText("bare");
    await user.click(screen.getByRole("button", { name: "Remove…" }));
    await screen.findByText(/Remove launch profile bare/);
    expect(ran("launch-profile", "rm", "bare", "--dry-run")).toBe(true);
    await user.click(screen.getByRole("button", { name: "Remove profile" }));
    await waitFor(() => expect(vi.mocked(toast.success)).toHaveBeenCalled());
    const [message, options] = vi.mocked(toast.success).mock.calls.at(-1)!;
    expect(message).toBe("Removed bare");
    (options as unknown as { action: { onClick: () => void } }).action.onClick();
    await waitFor(() =>
      expect(ran("launch-profile", "add", "bare", "--set", "work")).toBe(true),
    );
  });

  it("test06_the_folder_button_shows_only_while_the_policy_switch_is_on", async () => {
    setup(FOLDER);
    await screen.findByText("bare");
    expect(screen.queryByRole("button", { name: /Write to/ })).toBeNull();
    setup(FOLDER, {
      launchProfiles: { profiles: [{ name: "bare", set: "work", denyDirect: true }] },
    });
    const button = await screen.findByRole("button", { name: /Write to ~\/work\/proj/ });
    const user = userEvent.setup();
    await user.click(button);
    await screen.findByText(/Keep 0 direct entries out of/);
    expect(ran("launch-profile", "policy", "bare", "--cwd", FOLDER, "--dry-run")).toBe(
      true,
    );
  });
});
