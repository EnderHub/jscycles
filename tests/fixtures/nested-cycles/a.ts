// Cycle 1: a -> b -> a
// Cycle 2: a -> c -> d -> a (nested within the graph)
import { b } from "./b";
import { c } from "./c";

export const a = { b, c };
