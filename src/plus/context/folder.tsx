import { useCallback, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

const KEY = "toolport.context.recent-folders";
const LIMIT = 8;

/** A folder as a shell spells it, made plain: a terminal copies `'/a b/'` or `/a\ b/`, and
 * the command runs without a shell to take the quoting off. */
export function cleanFolder(raw: string): string {
  const text = raw.trim();
  const quoted = /^(['"])(.*)\1$/s.exec(text);
  if (quoted) return quoted[2];
  return text.replace(/\\ /g, " ");
}

function readRecent(): string[] {
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((item): item is string => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

/** The folders the person chose lately, newest first. The bridge has no native folder
 * dialog, so a path field with these as suggestions is the picker. Storage may be missing
 * (private window, blocked data): the list is then only what the screen already read. */
export function useRecentFolders() {
  const [recent, setRecent] = useState<string[]>(readRecent);
  const remember = useCallback((folder: string) => {
    const path = cleanFolder(folder);
    if (!path) return;
    setRecent((now) => {
      const next = [path, ...now.filter((one) => one !== path)].slice(0, LIMIT);
      try {
        window.localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        // the list simply is not kept
      }
      return next;
    });
  }, []);
  return { recent, remember };
}

export interface FolderChoice {
  draft: string;
  folder: string;
  setDraft: (value: string) => void;
  show: (value: string) => void;
}

/** The folder of the tab This folder, kept above the tab so switching tabs keeps it. It starts
 * from the newest recent folder, so a person returns to where they left off. */
export function useFolderChoice(): FolderChoice {
  const [state, setState] = useState(() => {
    const newest = readRecent()[0] ?? "";
    return { draft: newest, folder: newest };
  });
  const setDraft = useCallback(
    (draft: string) => setState((now) => ({ ...now, draft })),
    [],
  );
  const show = useCallback(
    (value: string) => setState({ draft: value, folder: cleanFolder(value) }),
    [],
  );
  return { ...state, setDraft, show };
}

export function suggestions(...lists: Array<string[] | undefined>): string[] {
  return [...new Set(lists.flatMap((list) => list ?? []))];
}

/** A folder path with suggestions; `onSubmit` runs when it is confirmed. */
export function FolderField({
  label = "Folder",
  value,
  onChange,
  options,
  placeholder,
  onSubmit,
  submitLabel,
}: {
  label?: string;
  value: string;
  onChange: (value: string) => void;
  options: string[];
  placeholder?: string;
  onSubmit?: () => void;
  submitLabel?: string;
}) {
  const id = useId();
  const list = `${id}-list`;
  const body = (
    <>
      <div className="flex min-w-[16rem] flex-1 flex-col gap-1.5">
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        <Input
          id={id}
          list={list}
          value={value}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => onChange(event.target.value)}
          onPaste={(event) => {
            const pasted = event.clipboardData.getData("text");
            const clean = cleanFolder(pasted);
            if (clean === pasted) return;
            event.preventDefault();
            const input = event.currentTarget;
            const start = input.selectionStart ?? value.length;
            const end = input.selectionEnd ?? value.length;
            onChange(value.slice(0, start) + clean + value.slice(end));
          }}
        />
        <datalist id={list}>
          {options.map((option) => (
            <option key={option} value={option} />
          ))}
        </datalist>
      </div>
      {onSubmit && (
        <Button type="submit" variant="outline">
          {submitLabel ?? "Show"}
        </Button>
      )}
    </>
  );
  return onSubmit ? (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      {body}
    </form>
  ) : (
    <div className="flex flex-wrap items-end gap-2">{body}</div>
  );
}
