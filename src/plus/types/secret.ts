import { bool, obj, opt, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the secret commands, checked against the golden envelopes by `data.test.ts`. */

export const secretGetData = obj({
  key: str,
  server: str,
  set: bool,
  value: opt(str),
});
export type SecretGetData = Infer<typeof secretGetData>;

export const secretRmData = obj({
  key: str,
  removed: bool,
  server: str,
});
export type SecretRmData = Infer<typeof secretRmData>;

/** Golden file stem to the shape of its envelope `data`. */
export const secretShapes: Record<string, Shape<unknown>> = {
  "secret-get.reveal": secretGetData,
  "secret-get.set": secretGetData,
  "secret-rm.apply": secretRmData,
};
