import type { ComponentType, ReactNode } from "react";

export type Children = { children?: ReactNode };
export type StackProps = Children & { gap?: number };
export type TextProps = Children & { kind?: "body" | "heading" | "muted" };
export type ButtonProps = Children & { onPress?: () => void; disabled?: boolean; submit?: boolean };
export type InputProps = {
  id: string;
  label: string;
  value: string;
  onValueChange: (value: string) => void;
  type?: "text" | "number";
  min?: number;
  max?: number;
  step?: string;
  required?: boolean;
  disabled?: boolean;
};
export type FormProps = Children & { onSubmit: () => void; busy?: boolean };
export type StatusProps = Children & { error?: boolean };
/** Host contract contains values and callbacks, with no DOM event or element types. */
export interface Host {
  Stack: ComponentType<StackProps>;
  Text: ComponentType<TextProps>;
  Button: ComponentType<ButtonProps>;
  Input: ComponentType<InputProps>;
  Form: ComponentType<FormProps>;
  Status: ComponentType<StatusProps>;
}
