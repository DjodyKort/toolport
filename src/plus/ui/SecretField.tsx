import { useId, useImperativeHandle, useRef, useState, type Ref } from "react";
import { Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

export interface SecretFieldHandle {
  /** The value, once; the field is empty afterwards. */
  take: () => string;
  clear: () => void;
}

interface Props {
  label: string;
  /** With a handler the field has a button: it gets the value once and the field is empty
   * afterwards. Without one the screen reads the value through `handle` when it runs. */
  onSubmit?: (value: string) => void | Promise<void>;
  handle?: Ref<SecretFieldHandle>;
  submitLabel?: string;
  hint?: string;
  /** Called with whether the field holds something, never with what. */
  onFilledChange?: (filled: boolean) => void;
  disabled?: boolean;
}

/** A write-only input for a secret. The browser owns the value: it is not in React state,
 * in a prop of anything below, in an event payload or in a log, it is never shown, and the
 * field is emptied as soon as the value is handed over. There is no reveal toggle. */
export function SecretField({
  label,
  onSubmit,
  handle,
  submitLabel = "Save",
  hint = "The value is written to the vault and never shown again.",
  onFilledChange,
  disabled,
}: Props) {
  const input = useRef<HTMLInputElement>(null);
  const id = useId();
  const [filled, setFilled] = useState(false);
  const [busy, setBusy] = useState(false);

  function setFilledTo(next: boolean) {
    setFilled(next);
    onFilledChange?.(next);
  }

  function take(): string {
    const field = input.current;
    const value = field?.value ?? "";
    if (field) field.value = "";
    setFilledTo(false);
    return value;
  }

  useImperativeHandle(handle, () => ({ take, clear: () => void take() }));

  async function submit() {
    if (!onSubmit || !filled) return;
    const value = take();
    if (value === "") return;
    setBusy(true);
    try {
      await onSubmit(value);
    } catch {
      // onSubmit owns surfacing its error; the field is already empty.
    } finally {
      setBusy(false);
    }
  }

  const body = (
    <>
      <label htmlFor={id} className="flex items-center gap-1.5 text-sm font-medium">
        <Lock className="size-3.5 text-muted-foreground" aria-hidden="true" /> {label}
      </label>
      <div className="flex gap-2">
        <Input
          id={id}
          ref={input}
          type="password"
          defaultValue=""
          onInput={() => setFilledTo((input.current?.value ?? "") !== "")}
          autoComplete="new-password"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          data-1p-ignore
          data-lpignore="true"
          aria-describedby={`${id}-hint`}
          disabled={disabled || busy}
        />
        {onSubmit && (
          <Button type="submit" disabled={disabled || busy || !filled}>
            {submitLabel}
          </Button>
        )}
      </div>
      <p id={`${id}-hint`} className="text-xs text-muted-foreground">
        {hint}
      </p>
    </>
  );
  return onSubmit ? (
    <form
      className="flex flex-col gap-1.5"
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      {body}
    </form>
  ) : (
    <div className="flex flex-col gap-1.5">{body}</div>
  );
}
