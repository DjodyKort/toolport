import { CtlError, ctlData } from "../bridge/ctl";
import type {
  AdoptEditData,
  CompressionDoctorData,
  CompressionStatusData,
  HookProviderId,
  HooksStatusData,
  HooksWriteData,
  MeasureReport,
  PauseData,
  VerifyData,
} from "./data";

const only = (provider?: HookProviderId) => (provider ? ["--provider", provider] : []);

export const hooksStatus = () =>
  ctlData<HooksStatusData>(["compression", "hooks", "status"]);

export const compressionStatus = () =>
  ctlData<CompressionStatusData>(["compression", "status"]);

export const compressionDoctor = () =>
  ctlData<CompressionDoctorData>(["compression", "doctor"]);

export const applyHooks = (provider?: HookProviderId, dryRun = false) =>
  ctlData<HooksWriteData>([
    "compression",
    "hooks",
    "apply",
    ...only(provider),
    ...(dryRun ? ["--dry-run"] : []),
  ]);

export const undoHooks = (provider?: HookProviderId, dryRun = false) =>
  ctlData<HooksWriteData>([
    "compression",
    "hooks",
    "undo",
    ...only(provider),
    ...(dryRun ? ["--dry-run"] : []),
  ]);

/** Pauses the wrapped hooks; with `duration` ("1d") they answer again by themselves. */
export const pauseHooks = (duration?: string) =>
  ctlData<PauseData>([
    "compression",
    "hooks",
    "pause",
    ...(duration ? ["--for", duration] : []),
  ]);

export const resumeHooks = () => ctlData<PauseData>(["compression", "hooks", "resume"]);

export function measureCompression(signal?: AbortSignal) {
  return ctlData<MeasureReport>(["compression", "measure"], { signal });
}

export const adoptEdit = (provider: HookProviderId, revert = false) =>
  ctlData<AdoptEditData>([
    "compression",
    "hooks",
    "adopt-edit",
    provider,
    ...(revert ? ["--revert"] : []),
  ]);

/** A failed check still answers with its rows, so the page can say which hook failed. */
export async function verifyHooks(provider?: HookProviderId): Promise<VerifyData> {
  try {
    return await ctlData<VerifyData>([
      "compression",
      "hooks",
      "verify",
      ...(provider ? [provider] : []),
    ]);
  } catch (error) {
    if (error instanceof CtlError && error.data) return error.data as VerifyData;
    throw error;
  }
}
