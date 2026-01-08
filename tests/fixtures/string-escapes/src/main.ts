// Import with various string styles
import { a } from "./a";
import { b } from './b';

// String with path-like content (should not be parsed as import)
const notImport = "import { x } from './fake'";
const alsoNotImport = 'import { y } from "./also-fake"';

export const main = { a, b };
