import { valueB } from '@nocycle/b';
import { valueC } from '@nocycle/c';

export const valueA = 'A imports B and C - no cycle';
export const result = valueA + valueB + valueC;
