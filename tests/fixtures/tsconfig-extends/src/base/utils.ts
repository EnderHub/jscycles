// Base utility - imports from app (creates cycle)
import { config } from "@app/config";

export function baseUtil() {
  return config;
}
