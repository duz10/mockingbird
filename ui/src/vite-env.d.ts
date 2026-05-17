/// <reference types="vite/client" />

// CSS Modules — return the class-name map so `styles.foo` is typed.
// Loose `string` typing keeps refactors cheap; the cost is no IDE
// auto-complete for class names. Trade-off accepted.
declare module "*.module.css" {
  const classes: Readonly<Record<string, string>>;
  export default classes;
}

declare module "*.module.scss" {
  const classes: Readonly<Record<string, string>>;
  export default classes;
}

// Plain CSS side-effect imports (`import "./tokens.css"`).
declare module "*.css";
