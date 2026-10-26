import type { Host } from "../../ui-primitives/src/types";

export const domHost: Host = {
  Stack: ({ children, gap = 16 }) => <div style={{ display: "flex", flexDirection: "column", gap }}>{children}</div>,
  Text: ({ children, kind }) => kind === "heading" ? <h1>{children}</h1> : <p className={kind === "muted" ? "muted" : undefined}>{children}</p>,
  Button: ({ children, onPress, disabled, submit }) => <button type={submit ? "submit" : "button"} disabled={disabled} onClick={onPress}>{children}</button>,
  Input: ({ label, onValueChange, ...props }) => <label htmlFor={props.id}>{label}<input {...props} onChange={(event) => onValueChange(event.currentTarget.value)} /></label>,
  Form: ({ children, onSubmit, busy }) => <form aria-busy={busy} onSubmit={(event) => { event.preventDefault(); onSubmit(); }}>{children}</form>,
  Status: ({ children, error }) => <div role={error ? "alert" : "status"} aria-live={error ? "assertive" : "polite"}>{children}</div>,
};
