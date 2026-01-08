// Long cycle: A -> B -> C -> D -> E -> A
import { b } from './b';
export const a = 'a' + b;
