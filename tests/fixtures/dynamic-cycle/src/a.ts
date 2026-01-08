// Cycle using dynamic import: A -> B (dynamic) -> A
export async function loadB() {
  const b = await import('./b');
  return b.value;
}
export const a = 'a';
