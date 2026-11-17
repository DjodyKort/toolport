import type { CommandRow, CommandToolRow, CommandsData } from "../bridge/data";

const envelopeFile = import.meta.glob(
  "../../../plus/fixtures/ctl-envelopes/commands/envelope.json",
  {
    eager: true,
    import: "default",
  },
);
const commandFiles = import.meta.glob(
  [
    "../../../plus/fixtures/ctl-envelopes/commands/*.json",
    "!../../../plus/fixtures/ctl-envelopes/commands/envelope.json",
  ],
  { eager: true, import: "default" },
);
const toolFiles = import.meta.glob(
  "../../../plus/fixtures/ctl-envelopes/commands/tools/*.json",
  {
    eager: true,
    import: "default",
  },
);

export interface CommandsGolden {
  argv: string[];
  exitCode: number;
  envelope: {
    command: string;
    ok: boolean;
    schemaVersion: number;
    data: CommandsData;
  };
}

type Head = Omit<CommandsGolden, "envelope"> & {
  envelope: Omit<CommandsGolden["envelope"], "data"> & {
    data: Omit<CommandsData, "commands" | "tools" | "counts">;
  };
};

const byKey = <T>(rows: T[], key: (row: T) => string) =>
  [...rows].sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));

/** The `toolportctl commands` golden rebuilt from `plus/fixtures/ctl-envelopes/commands/`, rows
 * sorted by id and name (the directory keeps no order; the Rust contract test asserts it). */
export function loadCommandsGolden(): CommandsGolden {
  const [head] = Object.values(envelopeFile) as [Head];
  const commands = byKey(Object.values(commandFiles) as CommandRow[], (row) => row.id);
  const tools = byKey(Object.values(toolFiles) as CommandToolRow[], (row) => row.name);
  const leaves = commands.filter((row) => row.kind === "command").length;
  return {
    ...head,
    envelope: {
      ...head.envelope,
      data: {
        ...head.envelope.data,
        commands,
        tools,
        counts: {
          rows: commands.length,
          commands: leaves,
          groups: commands.length - leaves,
          tools: tools.length,
        },
      },
    },
  };
}

export const commandsGolden = loadCommandsGolden();
