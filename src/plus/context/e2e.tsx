import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { ContextScreen } from "./ContextScreen";

export const section = (name: string) => screen.getByRole("region", { name });

/** Mounts the real screen without waiting for anything. */
export function mountContext(initialTab = "launch") {
  const user = userEvent.setup();
  render(<ContextScreen initialTab={initialTab} onOpenCommands={() => {}} />);
  return user;
}

/** Mounts the real screen and waits until every read has been answered. */
export async function openContext() {
  const user = mountContext();
  await screen.findByRole("tab", { name: "Launch & shell", selected: true });
  await waitFor(() => expect(document.querySelector('[aria-busy="true"]')).toBeNull());
  await screen.findByRole("list", { name: "Tokens per layer" });
  return user;
}

/** The review dialog of a write: the plan to confirm, with a typed phrase when the tier asks. */
export async function review(title: RegExp) {
  return within(await screen.findByRole("dialog", { name: title }));
}

/** Confirms the review dialog, waits for the result and closes it. */
export async function finish(
  user: UserEvent,
  title: RegExp,
  confirm: string,
  options: { typed?: string; done: string | RegExp },
) {
  const box = await review(title);
  if (options.typed) {
    expect(box.getByRole("button", { name: confirm })).toBeDisabled();
    await user.type(box.getByRole("textbox"), options.typed);
  }
  await user.click(box.getByRole("button", { name: confirm }));
  await screen.findByText(options.done);
  const done = screen.getByRole("dialog");
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}

/** Everything a person can read on the screen outside the text fields. */
export function visibleText(): string {
  const copy = document.body.cloneNode(true) as HTMLElement;
  copy.querySelectorAll("textarea, input").forEach((node) => node.remove());
  return copy.textContent ?? "";
}

/** Confirms the review dialog of a write, typing the phrase first when `typed` is given, and
 * returns the Result section of the apply. */
export async function confirmResult(
  user: UserEvent,
  title: RegExp,
  label: string,
  typed?: string,
) {
  const box = await review(title);
  if (typed !== undefined) {
    expect(box.getByRole("button", { name: label })).toBeDisabled();
    await user.type(box.getByLabelText(/Type .* to confirm/), typed);
  }
  await user.click(box.getByRole("button", { name: label }));
  return within(await screen.findByRole("region", { name: "Result" }));
}

/** Closes the result dialog of a write and waits until it is gone. */
export async function closeResult(user: UserEvent) {
  const done = screen.getByRole("dialog");
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}
