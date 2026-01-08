// Various comment patterns near imports
/* Block comment before */
import { a } from "./a"; // Inline comment

// Line comment before
import { b } from "./b";

/**
 * JSDoc before import
 */
import { c } from "./c";

// Commented out import should NOT be parsed:
// import { x } from "./x";

/* Also commented:
import { y } from "./y";
*/

export const main = { a, b, c };
