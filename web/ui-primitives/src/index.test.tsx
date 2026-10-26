import { afterEach, describe, expect, it } from "bun:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import type { Root } from "react-dom/client";
import { domHost } from "@loams/host-dom";
import { Button, Form, HostProvider, Input, Stack, Status, Text } from "./index";

let root: Root | undefined;
let window: Window | undefined;

afterEach(() => {
  if (root) act(() => root?.unmount());
  root = undefined;
  window?.document.defaultView?.close();
  window = undefined;
});

describe("host independent UI primitives", () => {
  it("maps input values, form submit, native labels, and live status on DOM", async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    window = new Window({ url: "http://localhost/" });
    Object.assign(globalThis, {
      window, document: window.document, navigator: window.navigator,
      HTMLElement: window.HTMLElement, Event: window.Event, MouseEvent: window.MouseEvent,
    });
    const container = window.document.createElement("div");
    window.document.body.append(container);
    const { createRoot } = await import("react-dom/client");
    root = createRoot(container as unknown as Element);
    let value = "start";
    let submitted = 0;
    const render = () => act(() => root?.render(
      <HostProvider host={domHost}><Stack gap={8}><Text kind="heading">Share</Text>
        <Form onSubmit={() => submitted++}><Input id="people" label="People" type="number" required value={value} onValueChange={(next) => { value = next; render(); }} /><Button submit>Calculate</Button></Form>
        <Status>Ready</Status></Stack></HostProvider>,
    ));
    render();
    const input = container.querySelector("#people") as unknown as HTMLInputElement;
    expect(input.labels?.[0]?.textContent).toBe("People");
    Object.getOwnPropertyDescriptor(window!.HTMLInputElement.prototype, "value")!.set!.call(input, "4");
    await act(async () => { input.dispatchEvent(new window!.Event("input", { bubbles: true }) as unknown as Event); input.dispatchEvent(new window!.Event("change", { bubbles: true }) as unknown as Event); });
    expect(value).toBe("4");
    await act(async () => { container.querySelector("form")!.dispatchEvent(new window!.Event("submit", { bubbles: true, cancelable: true })); });
    expect(submitted).toBe(1);
    expect(container.querySelector("h1")?.textContent).toBe("Share");
    expect(container.querySelector('[role="status"]')?.getAttribute("aria-live")).toBe("polite");
  });

  it("requires a host provider", async () => {
    window = new Window({ url: "http://localhost/" });
    Object.assign(globalThis, { window, document: window.document, HTMLElement: window.HTMLElement });
    const container = window.document.createElement("div");
    const { createRoot } = await import("react-dom/client");
    root = createRoot(container as unknown as Element);
    expect(() => act(() => root?.render(createElement(Text, null, "No host")))).toThrow("HostProvider");
  });
});
