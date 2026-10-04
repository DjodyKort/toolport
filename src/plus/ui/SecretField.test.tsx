import { describe, expect, it, vi } from "vitest";
import { createRef } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SecretField, type SecretFieldHandle } from "./SecretField";

const CANARY = "CANARY-secret-field-7be0";

describe("SecretField", () => {
  it("is a masked, write-only input with no reveal control", () => {
    render(<SecretField label="New value" onSubmit={vi.fn()} />);
    const field = screen.getByLabelText("New value");
    expect(field).toHaveAttribute("type", "password");
    expect(field).toHaveAttribute("autocomplete", "new-password");
    expect(screen.getByText(/never shown again/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /show|reveal|eye/i })).toBeNull();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("hands the value to onSubmit once and is empty afterwards", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    render(
      <SecretField label="New value" onSubmit={onSubmit} submitLabel="Save to vault" />,
    );
    const field = screen.getByLabelText("New value") as HTMLInputElement;
    await user.type(field, CANARY);
    expect(screen.getByRole("button", { name: "Save to vault" })).toBeEnabled();
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
    expect(onSubmit).toHaveBeenCalledWith(CANARY);
    expect(field.value).toBe("");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Save to vault" })).toBeDisabled(),
    );
  });

  it("empties the field even when saving fails", async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error("vault locked"));
    const user = userEvent.setup();
    render(<SecretField label="New value" onSubmit={onSubmit} />);
    const field = screen.getByLabelText("New value") as HTMLInputElement;
    await user.type(field, "abc");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(onSubmit).toHaveBeenCalledWith("abc");
    expect(field.value).toBe("");
    await waitFor(() => expect(field).toBeEnabled());
  });

  it("never puts the value in the DOM, the console or the filled callback", async () => {
    const spies = (["log", "info", "warn", "error", "debug"] as const).map((name) =>
      vi.spyOn(console, name).mockImplementation(() => {}),
    );
    const onFilledChange = vi.fn();
    const user = userEvent.setup();
    const { container } = render(
      <SecretField label="Secret" onSubmit={vi.fn()} onFilledChange={onFilledChange} />,
    );
    await user.type(screen.getByLabelText("Secret"), CANARY);
    expect(container.innerHTML).not.toContain(CANARY);
    expect(document.body.innerHTML).not.toContain(CANARY);
    expect(JSON.stringify(onFilledChange.mock.calls)).not.toContain(CANARY);
    expect(onFilledChange).toHaveBeenLastCalledWith(true);
    for (const spy of spies) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(CANARY);
      spy.mockRestore();
    }
  });

  it("lets a screen take the value through its handle, once, without a button", async () => {
    const handle = createRef<SecretFieldHandle>();
    const onFilledChange = vi.fn();
    const user = userEvent.setup();
    render(
      <SecretField label="Passphrase" handle={handle} onFilledChange={onFilledChange} />,
    );
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(document.querySelector("form")).toBeNull();
    const field = screen.getByLabelText("Passphrase") as HTMLInputElement;
    await user.type(field, CANARY);
    expect(handle.current!.take()).toBe(CANARY);
    expect(field.value).toBe("");
    expect(handle.current!.take()).toBe("");
    expect(onFilledChange).toHaveBeenLastCalledWith(false);
    await user.type(field, "again");
    handle.current!.clear();
    expect(field.value).toBe("");
  });

  it("can be disabled", () => {
    render(<SecretField label="Secret" onSubmit={vi.fn()} disabled />);
    expect(screen.getByLabelText("Secret")).toBeDisabled();
  });
});
