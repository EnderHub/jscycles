// Diamond dependency: top -> left, top -> right, left -> bottom, right -> bottom
// This is NOT a cycle
import { left } from './left';
import { right } from './right';
export const top = left + right;
