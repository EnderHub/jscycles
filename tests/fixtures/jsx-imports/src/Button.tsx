// Button component imports from App (creating cycle)
import React from "react";
import { App } from "./App";

export const Button = () => <button onClick={() => console.log(App)}>Click</button>;
