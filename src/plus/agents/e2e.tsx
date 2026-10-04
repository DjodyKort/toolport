import { expect } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { LibraryScreen } from "../skills/LibraryScreen";

/** Walks to a tab the way a person does: the Library screen and its Agents or Styles tab.
 * Everything after that runs through the real panels. */
export async function openFromLibrary(tab: "Agents" | "Styles") {
  const user = userEvent.setup();
  render(<LibraryScreen initialTab={tab.toLowerCase()} onOpenCommands={() => {}} />);
  await screen.findByRole("tab", { name: tab, selected: true });
  return user;
}

/** One confirmed write: press the button, wait for the plan, type the phrase when the tier
 * asks for one, confirm, wait for the result, close it. */
export async function write(
  user: UserEvent,
  button: string,
  confirm: string,
  options: { typed?: string; done: string | RegExp },
) {
  await user.click(screen.getByRole("button", { name: button }));
  const box = await screen.findByRole("dialog");
  await within(box).findByRole("button", { name: confirm });
  if (options.typed) {
    expect(within(box).getByRole("button", { name: confirm })).toBeDisabled();
    await user.type(within(box).getByRole("textbox"), options.typed);
  }
  await user.click(within(box).getByRole("button", { name: confirm }));
  await screen.findByText(options.done);
  const done = screen.getByRole("dialog");
  await user.click(within(done).getAllByRole("button", { name: "Close" }).at(-1)!);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
}
