// Utils imports from main, which imports from index, which re-exports utils
import { main } from "./main";

export function helper() {
  return main;
}
