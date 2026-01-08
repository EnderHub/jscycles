// Even checks via odd (mutual recursion pattern)
import { isOdd } from "./odd";

export function isEven(n: number): boolean {
  if (n === 0) return true;
  return isOdd(n - 1);
}
