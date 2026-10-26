use protox::prost::Message;
use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../proto");
    let source = root.join("loams/web/bills/v1/bills.proto");
    let ledger = root.join("field-tags.json");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", ledger.display());
    let descriptors = protox::compile([&source], [&root])?;
    let ledger: serde_json::Value = serde_json::from_slice(&fs::read(ledger)?)?;
    for file in &descriptors.file {
        if file.package.as_deref() != ledger["package"].as_str() {
            return Err("RPC ledger package mismatch".into());
        }
        if file.message_type.len()
            != ledger["messages"]
                .as_object()
                .ok_or("Missing RPC message ledger")?
                .len()
            || file.service.len()
                != ledger["services"]
                    .as_object()
                    .ok_or("Missing RPC service ledger")?
                    .len()
        {
            return Err("RPC message/service count differs from field-tag ledger".into());
        }
        for message in &file.message_type {
            let expected = ledger["messages"][message.name()]
                .as_object()
                .ok_or("Message missing from RPC field-tag ledger")?;
            if message.field.len() != expected.len() {
                return Err("RPC field count differs from field-tag ledger".into());
            }
            for field in &message.field {
                if field.label().as_str_name() != "LABEL_OPTIONAL"
                    || field.type_name.is_some()
                    || (field.oneof_index.is_some() && !field.proto3_optional())
                {
                    return Err("This opt-in RPC schema supports singular scalars and optional presence only".into());
                }
                let expected = expected
                    .get(field.name())
                    .ok_or("Field missing from RPC field-tag ledger")?;
                let kind = field
                    .r#type()
                    .as_str_name()
                    .strip_prefix("TYPE_")
                    .ok_or("Unknown proto field type")?
                    .to_ascii_lowercase();
                if expected["number"].as_i64() != Some(i64::from(field.number()))
                    || expected["type"].as_str() != Some(&kind)
                    || expected["optional"].as_bool().unwrap_or(false) != field.proto3_optional()
                {
                    return Err(format!(
                        "RPC field-tag ledger mismatch for {}.{}",
                        message.name(),
                        field.name()
                    )
                    .into());
                }
            }
        }
        for service in &file.service {
            if service.method.len()
                != ledger["services"][service.name()]
                    .as_object()
                    .ok_or("Missing RPC service ledger")?
                    .len()
            {
                return Err("RPC method count differs from service ledger".into());
            }
            for method in &service.method {
                let expected = &ledger["services"][service.name()][method.name()];
                let qualify = |value: &str| format!(".{}.{value}", file.package());
                if expected["input"].as_str().map(qualify).as_deref() != Some(method.input_type())
                    || expected["output"].as_str().map(qualify).as_deref()
                        != Some(method.output_type())
                    || method.client_streaming()
                    || method.server_streaming()
                {
                    return Err("RPC unary service ledger mismatch".into());
                }
            }
        }
    }
    let descriptor_path = PathBuf::from(env::var("OUT_DIR")?).join("bills.descriptor.bin");
    fs::write(&descriptor_path, descriptors.encode_to_vec())?;
    connectrpc_build::Config::new()
        .descriptor_set(descriptor_path)
        .files(&["loams/web/bills/v1/bills.proto"])
        .include_file("_connectrpc.rs")
        .emit_descriptor_set("bills.descriptor.bin")
        .gate_client_feature(true)
        .compile()?;
    Ok(())
}
