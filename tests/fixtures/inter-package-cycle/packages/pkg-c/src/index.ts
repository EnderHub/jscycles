import { valueA } from '@test/pkg-a';

export const valueC = 'C imports A - creates cycle!';
export const combinedC = valueC + valueA;
