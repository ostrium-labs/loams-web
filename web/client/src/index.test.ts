import { describe, expect, it } from "bun:test";
import { ApiClientError, createClient } from "../../app/src/generated/client";

describe("descriptor generated client", () => {
  it("sends the binding, signal, JSON body, and returns the typed response", async () => {
    let observed: RequestInit | undefined;
    const client = createClient("https://api.example", async (input, init) => {
      expect(input).toBe("https://api.example/split");
      observed = init;
      return Response.json({ each: 33 });
    });
    const controller = new AbortController();
    const result = await client["bill.split"](
      { total: 90, tip: 10, people: 3 },
      { signal: controller.signal },
    );
    expect(result).toEqual({ each: 33 });
    expect(observed?.method).toBe("POST");
    expect(observed?.headers).toBeInstanceOf(Headers);
    expect((observed?.headers as Headers).get("content-type")).toBe("application/json");
    expect(observed?.body).toBe(JSON.stringify({ total: 90, tip: 10, people: 3 }));
    expect(observed?.signal).toBe(controller.signal);
  });

  it("preserves structured validation payloads for callers", async () => {
    const errorBody = {
      code: "invalid_argument", reason: "invalid_input", message: "Input does not match the procedure schema",
      details: { problems: [{ at: "/people", code: "minimum", message: "Must be at least the minimum." }], truncated: false },
      request_id: "safe-id",
    };
    const client = createClient("", async () => Response.json(errorBody, { status: 400 }));
    let caught: unknown;
    try {
      await client["bill.split"]({ total: 90, tip: 10, people: 0 });
    } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(ApiClientError);
    expect(caught).toMatchObject({ status: 400, body: errorBody });
  });

  it("preserves text failures and rejects successful non-JSON responses", async () => {
    const client = createClient("", async () => new Response("upstream unavailable", { status: 503 }));
    try {
      await client["bill.split"]({ total: 90, tip: 10, people: 3 });
      throw new Error("request should fail");
    } catch (error) {
      expect(error).toMatchObject({ status: 503, body: "upstream unavailable" });
    }
    const success = createClient("", async () => new Response("not-json"));
    await expect(success["bill.split"]({ total: 90, tip: 10, people: 3 })).rejects.toThrow("invalid JSON");
  });
});
