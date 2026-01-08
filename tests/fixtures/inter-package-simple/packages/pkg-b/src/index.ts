import { valueA } from '@simple/a';

export const valueB = 'B imports A - cycle!';
export const result = valueB + valueA;
