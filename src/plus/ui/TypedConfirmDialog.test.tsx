import { describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TypedConfirmDialog } from "./TypedConfirmDialog";
import { PlanPreview } from "./PlanPreview";

function Harness({ onConfirm }: { onConfirm: () => void | Promise<void> }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      <button onClick={() => setOpen(true)}>Open</button>
      <TypedConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title="Remove acme-erp?"
        phrase="acme-erp"
        confirmLabel="Remove server"
        onConfirm={onConfirm}
      >
        <PlanPreview data={{ clients: ["claude-code"], dryRun: true }} />
      </TypedConfirmDialog>
    </>
  );
}

const confirmButton = () => screen.getByRole("button", { name: "Remove server" });

describe("TypedConfirmDialog", () => {
  it("shows the plan and keeps the button off until the phrase is typed exactly", async () => {
    const onConfirm = vi.fn();
    const user = userEvent.setup();
    render(<Harness onConfirm={onConfirm} />);
    expect(screen.getByRole("dialog", { name: "Remove acme-erp?" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Preview" })).toBeInTheDocument();
    expect(confirmButton()).toBeDisabled();

    const field = screen.getByRole("textbox", { name: /type acme-erp to confirm/i });
    await user.type(field, "odo");
    expect(confirmButton()).toBeDisabled();
    expect(field).toHaveAttribute("aria-invalid", "true");
    await user.type(field, "O");
    expect(confirmButton()).toBeDisabled();
    await user.clear(field);
    await user.type(field, "acme-erp");
    expect(confirmButton()).toBeEnabled();
    expect(field).not.toHaveAttribute("aria-invalid");
    await user.type(field, " ");
    expect(confirmButton()).toBeDisabled();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("confirms with the button and closes, but Enter in the field does not confirm", async () => {
    const onConfirm = vi.fn();
    const user = userEvent.setup();
    render(<Harness onConfirm={onConfirm} />);
    await user.type(screen.getByRole("textbox"), "acme-erp{Enter}");
    expect(onConfirm).not.toHaveBeenCalled();
    await user.click(confirmButton());
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("styles the confirm button as destructive and has a Cancel that closes it", async () => {
    const onConfirm = vi.fn();
    const user = userEvent.setup();
    render(<Harness onConfirm={onConfirm} />);
    expect(confirmButton()).toHaveAttribute("data-variant", "destructive");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onConfirm).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("closes on Escape and opens again with an empty field", async () => {
    const user = userEvent.setup();
    render(<Harness onConfirm={vi.fn()} />);
    await user.type(screen.getByRole("textbox"), "acme-erp");
    expect(confirmButton()).toBeEnabled();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("textbox")).toHaveValue("");
    expect(confirmButton()).toBeDisabled();
  });

  it("stays open when the action fails so the user can try again", async () => {
    const onConfirm = vi.fn().mockRejectedValue(new Error("nope"));
    const user = userEvent.setup();
    render(<Harness onConfirm={onConfirm} />);
    await user.type(screen.getByRole("textbox"), "acme-erp");
    await user.click(confirmButton());
    expect(onConfirm).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(confirmButton()).toBeEnabled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
