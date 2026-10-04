import { arr, bool, nullable, obj, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the cc commands, checked against the golden envelopes by `data.test.ts`. */

export const ccUpdateData = obj({
  mode: str,
  plugins: arr(
    obj({
      available: nullable(str),
      blocked: bool,
      enabled: bool,
      error: nullable(str),
      id: str,
      installed: str,
      marketplace: str,
      name: str,
      outcome: nullable(str),
      status: str,
    }),
  ),
  refreshError: nullable(str),
  restartRequired: bool,
});
export type CcUpdateData = Infer<typeof ccUpdateData>;

/** Golden file stem to the shape of its envelope `data`. */
export const ccShapes: Record<string, Shape<unknown>> = {
  "cc-update.apply": ccUpdateData,
  "cc-update.one": ccUpdateData,
  "cc-update.preview": ccUpdateData,
};
