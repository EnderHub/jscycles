import { valueA } from '@pnpm-test/a';

export const valueB = 'B from pnpm workspace - creates cycle';
export const result = valueB + valueA;
