# Loams Web macros

Enable the facade's `macros` feature and keep serde derives explicit:

```rust,ignore
#[derive(serde::Serialize, serde::Deserialize, loams_web::WireType)]
struct Bill {
    #[wire(min = 0.0)]
    total: f64,
    #[wire(min = 1, max = 50)]
    people: u32,
}

#[loams_web::procedure(name = "bill.split", method = "POST", path = "/split")]
async fn split(_ctx: loams_web::Ctx, bill: Bill) -> Result<f64, loams_web::ApiError> {
    Ok(bill.total / bill.people as f64)
}
```

`split_procedure()` builds the typed procedure. `split_procedure_builder()`
allows declaring additional errors or a summary before calling `build()`.
The original async function remains callable. These macros support free async
functions with two owned arguments and `Result<Output, ApiError>` results.
Names, methods, static paths and unsupported signatures are checked at expansion.

`WireType` implements `JsonSchema` and the marker trait using a private schema
type. Core validates that schema, so wire bounds and documented bounds have one
source. Do not also derive `JsonSchema`. Serde field names, directions, defaults,
skips, flattening, tagged enums, generics and recursive `Self` types retain their
schema behavior. Normal container defaults use the original `Default` value.
No direct schemars dependency is required by the generated code.

`wire(min = ..., max = ...)` supports finite numeric literals on numeric
primitives and optional numeric primitives. `wire(min_length = ...,
max_length = ...)` supports nonnegative integer literals on strings, standard
collections, arrays and optional versions. For aliases or advanced constraints,
use explicit `schemars` field attributes instead. Bounds never insert serde
derives or application validation callbacks.

Custom serde codecs require an explicit `schemars(with = "WireShape")` override,
whose correctness is the application's responsibility. Custom container default
functions and unions produce diagnostics. Generic type parameters require
`JsonSchema`; some custom defaults can require additional explicit bounds.
The facade dependency must currently retain its `loams_web` Rust crate name.
