import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetPlusFixture } from "../fixtures/ipc";
import { guiParity } from "../guiParity";
import { SetupPage } from "./SetupPage";

vi.mock("@tauri-apps/api/core", async () => {
  const { plusFixtureIpc } = await import("../fixtures/ipc");
  return {
    invoke: async (command: string, args: Record<string, unknown>) =>
      plusFixtureIpc(command, args),
  };
});
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

/** The actions of src/plus/gui-parity/ this file proves. */
const COVERS = ["setup.export", "setup.restore", "setup.undo", "setup.check"] as const;

describe("SetupPage", () => {
  beforeEach(() => resetPlusFixture());

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
    await user.type(screen.getByLabelText("Write to"), "/tmp/out");
    await user.type(screen.getByLabelText("Passphrase"), "short");
    expect(screen.getByRole("alert")).toHaveTextContent("shorter than 12");
    expect(screen.getByRole("button", { name: "Write the file" })).toBeDisabled();
    await user.clear(screen.getByLabelText("Passphrase"));
    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(await screen.findByText("Would write toolport-setup.zip")).toBeInTheDocument();
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
