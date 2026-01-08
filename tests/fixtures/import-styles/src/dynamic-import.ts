// Dynamic import
export async function load() {
  const mod = await import('./helper');
  return mod.helper;
}
