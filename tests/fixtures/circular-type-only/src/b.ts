// Type-only import back to a - completing type cycle
import type { AType } from "./a";

export interface BType {
  a: AType;
}

export const b = { name: "b" };
