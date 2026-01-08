// Helpers namespace imports back to utils - creating cycle
import * as utils from "./utils";

export const helper1 = { utils };
export const helper2 = { name: "helper2" };
