import { useCallback, useState } from "react";

const CURRENT_KEY = "toolport-plus-context-folder";
const RECENT_KEY = "toolport-plus-context-recent";
const RECENT_MAX = 12;

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The folder is a convenience: without storage the page asks again next time.
  }
}

function readRecent(): string[] {
  try {
    const parsed: unknown = JSON.parse(read(RECENT_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((p) => typeof p === "string") : [];
  } catch {
    return [];
  }
}

/** Makes `folder` the one the Context page opens on next. */
export function rememberFolder(folder: string) {
  write(CURRENT_KEY, folder);
}

export interface FolderChoice {
  folder: string | null;
  recent: string[];
  choose: (folder: string) => void;
}

/** The folder the three folder-aware tabs work on, and the folders chosen before. The app does not
 * know where Claude Code starts, so a folder is chosen once and remembered. */
export function useFolder(): FolderChoice {
  const [folder, setFolder] = useState<string | null>(() => read(CURRENT_KEY));
  const [recent, setRecent] = useState<string[]>(readRecent);
  const choose = useCallback((next: string) => {
    const path = next.trim().replace(/\/+$/, "") || "/";
    setFolder(path);
    setRecent((list) => {
      const updated = [path, ...list.filter((p) => p !== path)].slice(0, RECENT_MAX);
      write(RECENT_KEY, JSON.stringify(updated));
      return updated;
    });
    write(CURRENT_KEY, path);
  }, []);
  return { folder, recent, choose };
}
