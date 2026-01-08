// Local helper - imports from index (creates cycle)
import { util } from "@local/index";

export function helper() {
  return util;
}
