// Module A imports from barrel, creating cycle through re-exports
import { funcB } from "./index";

export function funcA() {
  return funcB();
}
