// Type-only import - creates a cycle at compile time
import type { BType } from "./b";

export interface AType {
  b: BType;
}

export const a = { name: "a" };
