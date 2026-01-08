// Static import back to A completes the cycle
import { a } from './a';
export const value = 'b' + a;
