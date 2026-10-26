import { createContext, useContext, type ReactNode } from "react";
import type { Host, StackProps, TextProps, ButtonProps, InputProps, FormProps, StatusProps } from "./types";
export type * from "./types";

const HostContext = createContext<Host | null>(null);
export function HostProvider({ host, children }: { host: Host; children?: ReactNode }) {
  return <HostContext.Provider value={host}>{children}</HostContext.Provider>;
}
function useHost() {
  const host = useContext(HostContext);
  if (!host) throw new Error("Loams primitives require a HostProvider");
  return host;
}
export function Stack(props: StackProps) { const View = useHost().Stack; return <View {...props} />; }
export function Text(props: TextProps) { const View = useHost().Text; return <View {...props} />; }
export function Button(props: ButtonProps) { const View = useHost().Button; return <View {...props} />; }
export function Input(props: InputProps) { const View = useHost().Input; return <View {...props} />; }
export function Form(props: FormProps) { const View = useHost().Form; return <View {...props} />; }
export function Status(props: StatusProps) { const View = useHost().Status; return <View {...props} />; }
