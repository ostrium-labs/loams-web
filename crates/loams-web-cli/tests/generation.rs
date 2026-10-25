use loams_web_cli::{ArtifactError, check_client, generate_typescript, write_client};
use loams_web_core::{HttpBinding, HttpMethod, ProcedureDescriptor};
use serde_json::{Value, json};

fn descriptor(name: &str, path: &str, input_schema: Value) -> ProcedureDescriptor {
    ProcedureDescriptor {
        name: name.into(),
        summary: None,
        http: Some(HttpBinding {
            method: HttpMethod::Post,
            path: path.into(),
        }),
        input_schema,
        output_schema: json!({"type":"string"}),
        errors: vec![],
    }
}

#[test]
fn generation_is_deterministic_and_encodes_fields_and_fetch_binding() {
    let first = descriptor(
        "z.last",
        "/last",
        json!({"type":"object","required":["kind"],"properties":{"kind":{"enum":["shared","private"]},"note":{"type":["string","null"]}},"additionalProperties":false}),
    );
    let second = descriptor(
        "a.first",
        "/first",
        json!({"type":"array","items":{"type":"integer"}}),
    );
    let source = generate_typescript(&[first.clone(), second.clone()]).unwrap();
    assert_eq!(source, generate_typescript(&[second, first]).unwrap());
    assert!(source.contains("Array<number>"));
    assert!(source.contains("\"kind\": \"shared\" | \"private\""));
    assert!(source.contains("\"note\"?: string | null"));
    assert!(source.contains("[\"z.last\"]: async"));
    assert!(source.contains("fetchImpl(base + \"/last\""));
    assert!(source.contains("if (!response.ok) throw new ApiClientError(response.status, body)"));
    assert!(source.contains("signal: options.signal"));
    assert!(source.contains("new Headers(options.headers)"));
}

#[test]
fn recursive_local_refs_maps_and_unions_have_named_types() {
    let schema = json!({"$ref":"#/$defs/Node", "$defs":{"Node":{"type":"object","properties":{"next":{"anyOf":[{"$ref":"#/$defs/Node"},{"type":"null"}]},"labels":{"type":"object","additionalProperties":{"type":"string"}}}}}});
    let source = generate_typescript(&[descriptor("tree", "/tree", schema)]).unwrap();
    assert!(source.contains("export type T_tree_Input = T_tree_Input_T_Node;"));
    assert!(source.contains("(T_tree_Input_T_Node) | (null)"));
    assert!(source.contains("Record<string, string>"));
}

#[test]
fn unsupported_or_invalid_schema_fails_instead_of_emitting_any() {
    for schema in [
        json!({"type":"array","prefixItems":[{"type":"string"}]}),
        json!({"type":"string","not":{"const":"x"}}),
        json!({"$ref":"https://invalid.example/schema"}),
        json!({"$ref":"#/$defs/Missing"}),
        json!({"enum":[{"value":1}]}),
        json!({"type":"object","patternProperties":{".*":{"type":"string"}}}),
    ] {
        assert!(generate_typescript(&[descriptor("bad", "/bad", schema)]).is_err());
    }
    let mixed = json!({"type":"object","properties":{"count":{"type":"number"}},"additionalProperties":{"type":"string"}});
    assert!(generate_typescript(&[descriptor("mixed", "/mixed", mixed)]).is_err());
}

#[test]
fn duplicate_routes_and_unsupported_http_paths_fail() {
    let base = descriptor("base", "/base", json!({"type":"null"}));
    assert!(generate_typescript(&[base.clone(), base.clone()]).is_err());
    let mut collision = base.clone();
    collision.name = "other".into();
    assert!(generate_typescript(&[base.clone(), collision]).is_err());
    for path in [
        "relative",
        "//outside",
        "/{id}",
        "/x?query=1",
        "/x#frag",
        "/\\outside",
    ] {
        let mut invalid = base.clone();
        invalid.http.as_mut().unwrap().path = path.into();
        assert!(generate_typescript(&[invalid]).is_err());
    }
    let mut get = base;
    get.http.as_mut().unwrap().method = HttpMethod::Get;
    assert!(generate_typescript(&[get]).is_err());
}

#[test]
fn dangerous_object_keys_and_colliding_punctuation_are_encoded() {
    let source = generate_typescript(&[
        descriptor("__proto__", "/proto", json!({"type":"null"})),
        descriptor("a.b", "/dot", json!({"type":"null"})),
        descriptor("a_2eb", "/underscore", json!({"type":"null"})),
    ])
    .unwrap();
    assert!(source.contains("[\"__proto__\"]: async"));
    assert!(source.contains("type T_a_2eb_Input"));
    assert!(source.contains("type T_a_5f2eb_Input"));
}

#[test]
fn artifact_check_detects_missing_stale_and_exact_output() {
    let directory =
        std::env::temp_dir().join(format!("loams-web-cli-artifacts-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let artifact = directory.join("client.ts");
    assert!(matches!(
        check_client(directory.join("missing.ts"), "source"),
        Err(ArtifactError::Io(_))
    ));
    write_client(&artifact, "source\n").unwrap();
    check_client(&artifact, "source\n").unwrap();
    assert!(matches!(
        check_client(&artifact, "changed\n"),
        Err(ArtifactError::Stale)
    ));
    std::fs::remove_file(artifact).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
