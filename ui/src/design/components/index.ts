// Barrel export for Design Language v1 component primitives.
// Import from "../design/components" rather than reaching into the
// individual files so renames stay cheap. Wave 3. ADR 0023.

export { Button } from "./Button";
export type { ButtonProps, ButtonVariant, ButtonSize } from "./Button";

export { Input } from "./Input";
export type { InputProps, InputVariant } from "./Input";

export { Switch } from "./Switch";
export type { SwitchProps } from "./Switch";

export { Chip } from "./Chip";
export type { ChipProps, ChipTone } from "./Chip";

export { Segmented } from "./Segmented";
export type { SegmentedProps, SegmentedOption } from "./Segmented";

export { ListItem } from "./ListItem";
export type { ListItemProps } from "./ListItem";

export { Dialog } from "./Dialog";
export type { DialogProps } from "./Dialog";

export { MockingbirdMark } from "./MockingbirdMark";
export type { MockingbirdMarkProps, MarkState } from "./MockingbirdMark";
