import {
  ChevronDown,
  Download,
  EllipsisVertical,
  FolderOpen,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { SummaryData } from "./data";
import {
  filterChips,
  filterNote,
  tildePath,
  UNKNOWN_PROJECT,
  type Filter,
  type FilterKind,
} from "./model";

const ALL = "";

export interface SpendHeaderProps {
  /** The folders of the range, unfiltered, so the list stays whole while one is chosen. */
  folders: string[];
  filter: Filter;
  onFolder: (folder: string | null) => void;
  onExport: () => void;
  onPrune: () => void;
}

/** The folder filter and Export CSV. At 390 px Export and Prune move into a More menu. */
export function SpendHeader({
  folders,
  filter,
  onFolder,
  onExport,
  onPrune,
}: SpendHeaderProps) {
  const label = filter.project ? tildePath(filter.project) : "All folders";
  return (
    <div
      className="flex flex-wrap items-center justify-between gap-3"
      data-slot="spend-header"
    >
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="outline"
            size="sm"
            aria-label={`Folder: ${label}. Change folder`}
            className="max-w-full"
          >
            <FolderOpen data-icon="inline-start" />
            <span className="max-w-55 truncate font-mono text-xs @max-[680px]:max-w-37.5">
              {label}
            </span>
            <ChevronDown data-icon="inline-end" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="max-h-80 overflow-y-auto">
          <DropdownMenuRadioGroup
            value={filter.project ?? ALL}
            onValueChange={(value) => onFolder(value === ALL ? null : value)}
          >
            <DropdownMenuRadioItem value={ALL}>All folders</DropdownMenuRadioItem>
            {folders
              .filter((folder) => folder !== UNKNOWN_PROJECT)
              .map((folder) => (
                <DropdownMenuRadioItem key={folder} value={folder}>
                  <span className="font-mono text-xs">{tildePath(folder)}</span>
                </DropdownMenuRadioItem>
              ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          className="hidden @min-[680px]:inline-flex"
          onClick={onExport}
        >
          <Download data-icon="inline-start" />
          Export CSV
          <kbd className="rounded border px-1 text-[10px] font-medium">E</kbd>
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="outline"
              size="icon"
              aria-label="More: Export CSV, Prune now"
              className="@min-[680px]:hidden"
            >
              <EllipsisVertical />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={onExport}>
              <Download />
              Export CSV
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={onPrune}>
              <Trash2 />
              Prune now…
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}

export interface FilterBarProps {
  filter: Filter;
  summary: SummaryData | null;
  whole: SummaryData | null;
  onClear: (kind: FilterKind) => void;
}

/** One chip per active filter and the sentence that says how much of the page it is. */
export function FilterBar({ filter, summary, whole, onClear }: FilterBarProps) {
  const chips = filterChips(filter);
  if (chips.length === 0) return null;
  const note = summary ? filterNote(summary, whole) : null;
  return (
    <div className="flex flex-wrap items-center gap-2" data-slot="filter-bar">
      {chips.map((chip) => (
        <span
          key={chip.kind}
          className="inline-flex items-center gap-1 rounded-full border bg-card py-0.5 pr-0.5 pl-3 text-meta"
        >
          <span className={chip.kind === "model" ? "" : "font-mono text-xs"}>
            {chip.label}
          </span>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Clear filter: ${chip.label}`}
            onClick={() => onClear(chip.kind)}
          >
            <X />
          </Button>
        </span>
      ))}
      {note && (
        <span className="text-meta text-muted-foreground">
          {note} <kbd className="rounded border px-1 text-[10px] font-medium">Esc</kbd>{" "}
          clears.
        </span>
      )}
    </div>
  );
}
