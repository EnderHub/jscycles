// Shared module - imports back to entry1 creating a cycle
import { entry1 } from "./entry1";

export const shared = { entry1 };
