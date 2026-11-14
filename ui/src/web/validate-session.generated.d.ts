// The types of validate-session.generated.js (web/gen-validate.mjs): the check of one line of a recording (testing-sheet-session/1).

export interface ValidateFunction {
  (doc: unknown): boolean;
  /** Why the last document was refused; only where and which rule, nothing of the document's text. */
  errors?: { instancePath: string; keyword: string; message?: string }[] | null;
}

export const validate: ValidateFunction;
export default validate;
