import { CtlError } from "../bridge/ctl";

export interface ErrorText {
  code: string | null;
  message: string;
}

/** What a screen shows for a failure: the CLI's own code and message for a failed
 * envelope, the message of anything else. Never the argv, stdin or stderr. */
export function errorText(error: unknown): ErrorText {
  if (error instanceof CtlError) return { code: error.code, message: error.message };
  if (error instanceof Error) return { code: null, message: error.message };
  return { code: null, message: typeof error === "string" ? error : "Unknown error" };
}

export function diagnosticsText(title: string, error: unknown, context?: string): string {
  const { code, message } = errorText(error);
  return [title, context, code ? `code: ${code}` : null, message]
    .filter((line): line is string => !!line)
    .join("\n");
}
