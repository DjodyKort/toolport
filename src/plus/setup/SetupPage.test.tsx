import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { invokeLog, ranArgv } from "../servers/testkit";
import { exportArgv } from "./model";
import { SetupPage } from "./SetupPage";

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
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

/** The actions of src/plus/gui-parity/ this file proves. */
const COVERS = ["setup.export", "setup.restore", "setup.undo", "setup.check"] as const;

describe("SetupPage", () => {
  beforeEach(() => {
    resetPlusFixture();
    invokeLog.length = 0;
  });

  it("names every action it covers as built", () => {
    for (const id of COVERS) {
      expect(guiParity.actions[id]).toMatchObject({
        status: "built",
        test: "src/plus/setup/SetupPage.test.tsx",
      });
    }
  });

  it("previews and writes an export, asking for a long enough passphrase", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await screen.findByLabelText(/Repositories/);
    expect(screen.queryByLabelText("Passphrase")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.click(screen.getByLabelText(/Secrets/));
    await user.type(screen.getByLabelText("Passphrase"), "short");
    expect(screen.getByRole("alert")).toHaveTextContent("shorter than 12");
    expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    await user.clear(screen.getByLabelText("Passphrase"));
    expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(await screen.findByText("Would write toolport-setup.zip")).toBeInTheDocument();
  });

  it("test01_a_typed_passphrase_exports_no_secrets_while_secrets_stay_unticked", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await screen.findByLabelText(/Repositories/);
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.click(screen.getByLabelText(/Secrets/));
    await user.type(screen.getByLabelText("Passphrase"), "a long enough passphrase");
    await user.click(screen.getByLabelText(/Secrets/));
    expect(screen.queryByLabelText("Passphrase")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Write the file" }));
    await waitFor(() => expect(ranArgv().some((a) => a[1] === "export")).toBe(true));
    const argv = ranArgv().find((a) => a[1] === "export")!;
    expect(argv).not.toContain("--secrets");
    expect(argv).not.toContain("--passphrase-stdin");
    expect(argv[argv.indexOf("--parts") + 1]).not.toMatch(/secrets|history/);
    const sent = invokeLog
      .filter((e) => e.command === "plus_ctl")
      .map((e) => e.args as { stdinSecret: string | null });
    expect(sent.every((a) => !a.stdinSecret)).toBe(true);
  });

  it("test02_ticking_secrets_sends_the_flag_and_the_passphrase_on_stdin", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await screen.findByLabelText(/Repositories/);
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.click(screen.getByLabelText(/Secrets/));
    await user.type(screen.getByLabelText("Passphrase"), "a long enough passphrase");
    await user.click(screen.getByRole("button", { name: "Write the file" }));
    await waitFor(() => expect(ranArgv().some((a) => a[1] === "export")).toBe(true));
    const argv = ranArgv().find((a) => a[1] === "export")!;
    expect(argv).toContain("--secrets");
    expect(argv).toContain("--passphrase-stdin");
    expect(argv[argv.indexOf("--parts") + 1].split(",")).toContain("secrets");
    const call = invokeLog
      .filter((e) => e.command === "plus_ctl")
      .map((e) => e.args as { argv: string[]; stdinSecret: string | null })
      .find((a) => a.argv[1] === "export")!;
    expect(call.stdinSecret).toBe("a long enough passphrase");
  });

  it("test03_sessions_need_the_passphrase_but_never_switch_secrets_on", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await screen.findByLabelText(/Repositories/);
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.click(screen.getByLabelText(/Sessions/));
    expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    await user.type(screen.getByLabelText("Passphrase"), "a long enough passphrase");
    await user.click(screen.getByRole("button", { name: "Write the file" }));
    await waitFor(() => expect(ranArgv().some((a) => a[1] === "export")).toBe(true));
    const argv = ranArgv().find((a) => a[1] === "export")!;
    expect(argv).toContain("--passphrase-stdin");
    expect(argv).not.toContain("--secrets");
  });

  it("test04_the_export_argv_takes_secrets_from_the_parts_alone", () => {
    const plain = exportArgv(["repos"], "/o", { passphrase: false, dryRun: false });
    expect(plain).not.toContain("--secrets");
    expect(plain).not.toContain("--passphrase-stdin");
    const sessions = exportArgv(["repos", "history"], "/o", {
      passphrase: true,
      dryRun: false,
    });
    expect(sessions).toContain("--passphrase-stdin");
    expect(sessions).not.toContain("--secrets");
    expect(exportArgv(["secrets"], "/o", { passphrase: true, dryRun: true })).toEqual([
      "setup",
      "export",
      "--parts",
      "secrets",
      "--out",
      "/o",
      "--secrets",
      "--passphrase-stdin",
      "--dry-run",
    ]);
  });

  it("shows the plan first, restores with its hash and undoes it", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await user.click(screen.getByRole("tab", { name: "Restore" }));
    await user.type(screen.getByLabelText("Setup file"), "/tmp/setup.zip");
    await user.click(screen.getByRole("button", { name: "Check the file" }));
    expect(await screen.findByText("Will be added")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore" }));
    expect(await screen.findByText("Restored")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Undo the last restore" }));
    await waitFor(() => expect(screen.queryByText("Restored")).not.toBeInTheDocument());
  });

  it("ticks what you carry over and acknowledges a blocker", async () => {
    const user = userEvent.setup();
    render(<SetupPage onBack={() => {}} />);
    await user.click(screen.getByRole("tab", { name: "Before a reset" }));
    expect(await screen.findByText("Not ready for a reset")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Acknowledge" }));
    await user.click(await screen.findByLabelText("browser"));
    await waitFor(() => expect(screen.getByLabelText("browser")).toBeChecked());
  });
});
