// Odd checks via even (mutual recursion pattern)
import { isEven } from "./even";

export function isOdd(n: number): boolean {
  if (n === 0) return false;
  return isEven(n - 1);
}
