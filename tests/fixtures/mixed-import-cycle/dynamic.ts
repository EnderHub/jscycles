// Dynamic import creates cycle back to static
export async function dynamic() {
  const mod = await import("./require");
  return mod;
}
