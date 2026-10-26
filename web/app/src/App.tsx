import { useState } from "react";
import { useMutation } from "@loams/web";
import { Stack, Text, Input, Button, Form, Status } from "@loams/react";
import { createClient, ApiClientError } from "./generated/client";

const client = createClient("");
type Problem = { at: string; message: string };
function errorText(error: unknown): string {
  if (error instanceof ApiClientError && typeof error.body === "object" && error.body !== null) {
    const body = error.body as { message?: string; details?: { problems?: Problem[] } };
    if (body.details?.problems?.length) return body.details.problems.map(p => `${p.at || "Input"}: ${p.message}`).join(" ");
    return body.message ?? "The request could not be completed.";
  }
  return "Unable to reach the server. Please try again.";
}
export function App() {
  const [total, setTotal] = useState("90");
  const [tip, setTip] = useState("10");
  const [people, setPeople] = useState("3");
  const { state, execute } = useMutation(client["bill.split"]);
  const busy = state.status === "pending";
  return <Stack gap={24}>
    <Text kind="muted">LOAMS WEB</Text>
    <Text kind="heading">A fair share.</Text>
    <Text>Split a bill, including the tip, with everyone at the table.</Text>
    <Form busy={busy} onSubmit={() => { void execute({ total: Number(total), tip: Number(tip), people: Number(people) }).catch(() => {}); }}>
      <Stack gap={18}>
        <Input id="total" label="Bill total" type="number" min={0} step="0.01" required value={total} onValueChange={setTotal} disabled={busy} />
        <Input id="tip" label="Tip (%)" type="number" min={0} max={100} step="1" required value={tip} onValueChange={setTip} disabled={busy} />
        <Input id="people" label="People" type="number" min={1} max={50} step="1" required value={people} onValueChange={setPeople} disabled={busy} />
        <Button submit disabled={busy}>{busy ? "Calculating…" : "Calculate each share"}</Button>
      </Stack>
    </Form>
    <Status error={state.status === "error"}>
      {state.status === "success" ? <><strong>{state.data.each.toFixed(2)}</strong> per person</> : state.status === "error" ? errorText(state.error) : busy ? "Calculating your share…" : "Ready when you are."}
    </Status>
    <Text kind="muted">One typed procedure. The same validation in the API and its schema.</Text>
  </Stack>;
}
