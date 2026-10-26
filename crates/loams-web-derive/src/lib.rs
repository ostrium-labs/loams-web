//! Procedure factories and schema derives for Loams Web.
//!
//! `WireType` emits `JsonSchema` and a marker implementation. It preserves serde
//! shapes and puts `wire` bounds in that same schema, which core validates.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, FnArg, GenericArgument, ItemFn, Lit, LitStr, Meta,
    PathArguments, ReturnType, Type, parse_macro_input, parse_quote, visit_mut::VisitMut,
};

#[proc_macro_attribute]
/// Preserve an async handler and generate `<name>_procedure` plus an extensible
/// `<name>_procedure_builder` factory.
///
/// ```compile_fail
/// use loams_web_derive::procedure;
/// #[procedure(name = "bad", method = "POST", path = "/bad")]
/// fn bad(ctx: loams_web_core::Ctx, input: u32) -> Result<u32,loams_web_core::ApiError> {
///     Ok(input)
/// }
/// ```
pub fn procedure(attribute: TokenStream, input: TokenStream) -> TokenStream {
    let options = parse_macro_input!(attribute with syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated);
    let function = parse_macro_input!(input as ItemFn);
    expand_procedure(options.into_iter().collect(), function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn string_option(meta: &Meta) -> syn::Result<String> {
    if let Meta::NameValue(value) = meta
        && let Expr::Lit(value) = &value.value
        && let Lit::Str(value) = &value.lit
    {
        return Ok(value.value());
    }
    Err(syn::Error::new_spanned(
        meta,
        "procedure options must be string literals",
    ))
}

fn expand_procedure(options: Vec<Meta>, function: ItemFn) -> syn::Result<Tokens> {
    let mut name = None;
    let mut method = None;
    let mut path = None;
    for option in options {
        let target = if option.path().is_ident("name") {
            &mut name
        } else if option.path().is_ident("method") {
            &mut method
        } else if option.path().is_ident("path") {
            &mut path
        } else {
            return Err(syn::Error::new_spanned(
                option,
                "unknown procedure option; expected name, method or path",
            ));
        };
        if target.is_some() {
            return Err(syn::Error::new_spanned(
                option,
                "duplicate procedure option",
            ));
        }
        *target = Some(string_option(&option)?);
    }
    let name = name.ok_or_else(|| {
        syn::Error::new(
            function.sig.ident.span(),
            "procedure requires name = \"stable.name\"",
        )
    })?;
    let method = method.ok_or_else(|| {
        syn::Error::new(
            function.sig.ident.span(),
            "procedure requires method = \"POST\"",
        )
    })?;
    let path = path.ok_or_else(|| {
        syn::Error::new(
            function.sig.ident.span(),
            "procedure requires path = \"/path\"",
        )
    })?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(syn::Error::new(
            function.sig.ident.span(),
            "procedure name must contain ASCII letters, digits, dot, underscore or hyphen",
        ));
    }
    let method = match method.as_str() {
        "GET" => quote!(Get),
        "HEAD" => quote!(Head),
        "POST" => quote!(Post),
        "PUT" => quote!(Put),
        "PATCH" => quote!(Patch),
        "DELETE" => quote!(Delete),
        "OPTIONS" => quote!(Options),
        _ => {
            return Err(syn::Error::new(
                function.sig.ident.span(),
                "unsupported procedure method; use uppercase GET, HEAD, POST, PUT, PATCH, DELETE or OPTIONS",
            ));
        }
    };
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '{', '}', ':', '*', '\\'])
        || path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(syn::Error::new(
            function.sig.ident.span(),
            "procedure path must be a static absolute path without query, fragment or parameters",
        ));
    }
    let sig = &function.sig;
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            sig,
            "procedure handler must be async",
        ));
    }
    if sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            sig,
            "procedure handlers cannot be generic, unsafe, const, extern or variadic",
        ));
    }
    if sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &sig.inputs,
            "procedure requires exactly two arguments: ctx: Ctx and input: Input",
        ));
    }
    let mut inputs = sig.inputs.iter();
    let context = match inputs.next().expect("two inputs checked") {
        FnArg::Typed(input) => input,
        FnArg::Receiver(input) => {
            return Err(syn::Error::new_spanned(
                input,
                "procedure handlers cannot have a self receiver",
            ));
        }
    };
    if !type_named(&context.ty, "Ctx") {
        return Err(syn::Error::new_spanned(
            &context.ty,
            "first procedure argument must be Ctx by value",
        ));
    }
    let input = match inputs.next().expect("two inputs checked") {
        FnArg::Typed(input) => &input.ty,
        FnArg::Receiver(input) => {
            return Err(syn::Error::new_spanned(
                input,
                "procedure handlers cannot have a self receiver",
            ));
        }
    };
    if matches!(input.as_ref(), Type::Reference(_)) {
        return Err(syn::Error::new_spanned(
            input,
            "procedure input must be an owned value",
        ));
    }
    let output = match &sig.output {
        ReturnType::Type(_, output) => output,
        _ => {
            return Err(syn::Error::new_spanned(
                sig,
                "procedure handler must return Result<Output, ApiError>",
            ));
        }
    };
    let Type::Path(output_path) = output.as_ref() else {
        return Err(syn::Error::new_spanned(
            output,
            "procedure handler must return Result<Output, ApiError>",
        ));
    };
    let segment = output_path.path.segments.last().ok_or_else(|| {
        syn::Error::new_spanned(
            output,
            "procedure handler must return Result<Output, ApiError>",
        )
    })?;
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            output,
            "procedure handler must return Result<Output, ApiError>",
        ));
    };
    let values = arguments.args.iter().collect::<Vec<_>>();
    if segment.ident != "Result" || values.len() != 2 {
        return Err(syn::Error::new_spanned(
            output,
            "procedure handler must return Result<Output, ApiError>",
        ));
    }
    let (GenericArgument::Type(output), GenericArgument::Type(error)) = (values[0], values[1])
    else {
        return Err(syn::Error::new_spanned(
            arguments,
            "procedure handler must return Result<Output, ApiError>",
        ));
    };
    if !type_named(error, "ApiError") {
        return Err(syn::Error::new_spanned(
            error,
            "procedure error type must be ApiError",
        ));
    }
    let ident = &sig.ident;
    let factory = format_ident!("{}_procedure", ident);
    let builder = format_ident!("{}_procedure_builder", ident);
    let visibility = &function.vis;
    let cfg = function
        .attrs
        .iter()
        .filter(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        })
        .collect::<Vec<_>>();
    Ok(quote! {
        #function
        #(#cfg)*
        #visibility fn #factory() -> ::std::result::Result<::loams_web::Procedure<#input,#output>, ::loams_web::DefinitionError> {
            #builder().build()
        }
        #(#cfg)*
        #visibility fn #builder() -> ::loams_web::ProcedureBuilder<#input,#output> {
            ::loams_web::Procedure::<#input,#output>::builder(#name)
                .http(::loams_web::HttpMethod::#method, #path)
                .handler(#ident)
        }
    })
}

fn type_named(ty: &Type, expected: &str) -> bool {
    matches!(ty,Type::Path(path) if path.qself.is_none() && path.path.segments.last().is_some_and(|segment| segment.ident == expected && matches!(segment.arguments,PathArguments::None)))
}

#[proc_macro_derive(WireType, attributes(wire, serde, schemars))]
/// Derive the schema used by both invocation and documentation. Serde derives
/// remain explicit. Numeric and length bounds are finite literals; unsupported
/// custom codecs require an explicit `schemars(with = "WireShape")` override.
///
/// ```compile_fail
/// use loams_web_derive::WireType;
/// #[derive(WireType)]
/// struct Invalid {
///     #[wire(min = 1)]
///     name: String,
/// }
/// ```
pub fn wire_type(input: TokenStream) -> TokenStream {
    expand_wire(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn has_setting(attrs: &[Attribute], namespace: &str, setting: &str) -> bool {
    attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident(namespace))
        .any(|attribute| {
            attribute
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|metas| metas.iter().any(|meta| meta.path().is_ident(setting)))
        })
}

fn check_codecs(attrs: &[Attribute]) -> syn::Result<()> {
    for attribute in attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        let options = attribute.parse_args_with(
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
        )?;
        for option in options {
            if [
                "with",
                "serialize_with",
                "deserialize_with",
                "from",
                "try_from",
                "into",
                "remote",
            ]
            .iter()
            .any(|setting| option.path().is_ident(setting))
                && !has_setting(attrs, "schemars", "with")
            {
                return Err(syn::Error::new_spanned(
                    option,
                    "custom serde representations require an explicit #[schemars(with = \"WireShape\")] override",
                ));
            }
        }
    }
    Ok(())
}

fn retain_schema_attributes(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attribute| {
        ["serde", "schemars", "doc", "cfg", "cfg_attr"]
            .iter()
            .any(|name| attribute.path().is_ident(name))
    });
}

struct ReplaceSelf {
    original: Type,
}
impl VisitMut for ReplaceSelf {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if matches!(ty,Type::Path(path) if path.qself.is_none() && path.path.is_ident("Self")) {
            *ty = self.original.clone();
        } else {
            syn::visit_mut::visit_type_mut(self, ty);
        }
    }
}

fn expand_wire(input: DeriveInput) -> syn::Result<Tokens> {
    if matches!(input.data, Data::Union(_)) {
        return Err(syn::Error::new_spanned(
            input.ident,
            "WireType does not support unions",
        ));
    }
    if input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("derive"))
        .any(|attribute| {
            attribute
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|paths| {
                    paths.iter().any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "JsonSchema")
                    })
                })
        })
    {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "WireType already implements JsonSchema; remove the separate JsonSchema derive",
        ));
    }
    if let Some(attribute) = input
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("wire"))
    {
        return Err(syn::Error::new_spanned(
            attribute,
            "wire bounds belong on fields",
        ));
    }
    check_codecs(&input.attrs)?;
    if has_setting(&input.attrs, "schemars", "crate") {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "WireType manages the schemars crate path; remove the crate override",
        ));
    }
    let container_default = has_setting(&input.attrs, "serde", "default");
    if container_default {
        if !matches!(input.data, Data::Struct(_)) {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "container serde(default) is supported only on structs",
            ));
        }
        for attribute in input
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("serde"))
        {
            let options = attribute.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            )?;
            if options
                .iter()
                .any(|option| option.path().is_ident("default") && !matches!(option, Meta::Path(_)))
            {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "custom container defaults are unsupported; use field defaults or implement JsonSchema explicitly",
                ));
            }
        }
    }
    let original = &input.ident;
    let shadow_ident = format_ident!("__LoamsWireSchema_{}", original);
    let mut shadow = input.clone();
    shadow.ident = shadow_ident.clone();
    shadow.vis = syn::Visibility::Inherited;
    retain_schema_attributes(&mut shadow.attrs);
    shadow
        .attrs
        .push(parse_quote!(#[allow(non_camel_case_types, dead_code)]));
    shadow.attrs.insert(
        0,
        parse_quote!(#[derive(::loams_web::__private::schemars::JsonSchema)]),
    );
    shadow
        .attrs
        .push(parse_quote!(#[schemars(crate = "::loams_web::__private::schemars")]));
    if !has_setting(&input.attrs, "schemars", "rename")
        && !has_setting(&input.attrs, "serde", "rename")
    {
        let mut name = original.to_string();
        for parameter in input.generics.type_params() {
            name.push_str(&format!("_{{{}}}", parameter.ident));
        }
        let name = LitStr::new(&name, original.span());
        shadow.attrs.push(parse_quote!(#[schemars(rename = #name)]));
    }
    let (_, original_args, _) = input.generics.split_for_impl();
    let mut rewrite = ReplaceSelf {
        original: syn::parse2(quote!(#original #original_args))?,
    };
    match &mut shadow.data {
        Data::Struct(data) => transform_fields(&mut data.fields, &mut rewrite)?,
        Data::Enum(data) => {
            for variant in &mut data.variants {
                if let Some(attribute) = variant
                    .attrs
                    .iter()
                    .find(|attribute| attribute.path().is_ident("wire"))
                {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "wire bounds belong on fields",
                    ));
                }
                check_codecs(&variant.attrs)?;
                retain_schema_attributes(&mut variant.attrs);
                transform_fields(&mut variant.fields, &mut rewrite)?;
            }
        }
        Data::Union(_) => unreachable!(),
    }
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter
            .bounds
            .push(parse_quote!(::loams_web::__private::schemars::JsonSchema));
    }
    if container_default {
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#original #original_args: ::std::default::Default));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let default_impl = if container_default {
        forward_default(&input, &shadow_ident)?
    } else {
        Tokens::new()
    };
    Ok(quote! {
        const _: () = {
            #shadow
            #default_impl
            impl #impl_generics ::loams_web::__private::schemars::JsonSchema for #original #type_generics #where_clause {
                fn inline_schema() -> bool { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::inline_schema() }
                fn schema_name() -> ::std::borrow::Cow<'static,str> { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::schema_name() }
                fn schema_id() -> ::std::borrow::Cow<'static,str> { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::schema_id() }
                fn json_schema(generator: &mut ::loams_web::__private::schemars::SchemaGenerator) -> ::loams_web::__private::schemars::Schema { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::json_schema(generator) }
                fn _schemars_private_non_optional_json_schema(generator: &mut ::loams_web::__private::schemars::SchemaGenerator) -> ::loams_web::__private::schemars::Schema { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::_schemars_private_non_optional_json_schema(generator) }
                fn _schemars_private_is_option() -> bool { <#shadow_ident #type_generics as ::loams_web::__private::schemars::JsonSchema>::_schemars_private_is_option() }
            }
            impl #impl_generics ::loams_web::WireType for #original #type_generics #where_clause {}
        };
    })
}

fn forward_default(input: &DeriveInput, shadow: &syn::Ident) -> syn::Result<Tokens> {
    let original = &input.ident;
    let mut generics = input.generics.clone();
    let (_, ty, _) = input.generics.split_for_impl();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(#original #ty: ::std::default::Default));
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let Data::Struct(data) = &input.data else {
        unreachable!()
    };
    let convert = match &data.fields {
        Fields::Named(fields) => {
            let names = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().expect("named field"))
                .collect::<Vec<_>>();
            quote!(let #original { #(#names),* } = value; Self { #(#names),* })
        }
        Fields::Unnamed(fields) => {
            let names = (0..fields.unnamed.len())
                .map(|index| format_ident!("field_{index}"))
                .collect::<Vec<_>>();
            quote!(let #original(#(#names),*) = value; Self(#(#names),*))
        }
        Fields::Unit => quote!(let _ = value; Self),
    };
    Ok(
        quote!(impl #impl_generics ::std::default::Default for #shadow #type_generics #where_clause { fn default() -> Self { let value = <#original #type_generics as ::std::default::Default>::default(); #convert } }),
    )
}

fn transform_fields(fields: &mut Fields, rewrite: &mut ReplaceSelf) -> syn::Result<()> {
    for field in fields.iter_mut() {
        check_codecs(&field.attrs)?;
        let mut range = Vec::new();
        let mut length = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut numeric_bounds = std::collections::HashMap::new();
        let mut length_bounds = std::collections::HashMap::new();
        for attribute in field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("wire"))
        {
            let options = attribute.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            )?;
            if options.is_empty() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "wire requires at least one bound",
                ));
            }
            for option in options {
                let key = option
                    .path()
                    .get_ident()
                    .map(ToString::to_string)
                    .ok_or_else(|| syn::Error::new_spanned(&option, "unsupported wire bound"))?;
                if !seen.insert(key.clone()) {
                    return Err(syn::Error::new_spanned(option, "duplicate wire bound"));
                }
                let Meta::NameValue(value) = option else {
                    return Err(syn::Error::new_spanned(
                        option,
                        "wire bounds require numeric literals",
                    ));
                };
                match key.as_str() {
                    "min" | "max" => {
                        numeric_bounds.insert(key.clone(), numeric_literal(&value.value)?);
                        if !numeric_type(&field.ty) {
                            return Err(syn::Error::new_spanned(
                                &field.ty,
                                "wire min/max require a numeric primitive or Option of one",
                            ));
                        }
                        let name = value.path;
                        let value = value.value;
                        range.push(quote!(#name = #value));
                    }
                    "min_length" | "max_length" => {
                        let Expr::Lit(expression) = &value.value else {
                            return Err(syn::Error::new_spanned(
                                value,
                                "wire length bounds require nonnegative integer literals",
                            ));
                        };
                        let Lit::Int(literal) = &expression.lit else {
                            return Err(syn::Error::new_spanned(
                                value,
                                "wire length bounds require nonnegative integer literals",
                            ));
                        };
                        length_bounds.insert(
                            key.clone(),
                            literal.base10_parse::<u64>().map_err(|_| {
                                syn::Error::new_spanned(
                                    literal,
                                    "wire length bounds require nonnegative integer literals",
                                )
                            })?,
                        );
                        if !length_type(&field.ty) {
                            return Err(syn::Error::new_spanned(
                                &field.ty,
                                "wire length bounds require String, a standard collection, array, or Option of one",
                            ));
                        }
                        let name = if key == "min_length" {
                            quote!(min)
                        } else {
                            quote!(max)
                        };
                        let value = value.value;
                        length.push(quote!(#name = #value));
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            value,
                            "unsupported wire bound; expected min, max, min_length or max_length",
                        ));
                    }
                }
            }
        }
        if numeric_bounds
            .get("min")
            .zip(numeric_bounds.get("max"))
            .is_some_and(|(min, max)| min > max)
        {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "wire min cannot exceed max",
            ));
        }
        if length_bounds
            .get("min_length")
            .zip(length_bounds.get("max_length"))
            .is_some_and(|(min, max)| min > max)
        {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "wire min_length cannot exceed max_length",
            ));
        }
        if !range.is_empty() && has_setting(&field.attrs, "schemars", "range") {
            return Err(syn::Error::new_spanned(
                field,
                "use wire bounds or schemars(range), not both",
            ));
        }
        if !length.is_empty() && has_setting(&field.attrs, "schemars", "length") {
            return Err(syn::Error::new_spanned(
                field,
                "use wire bounds or schemars(length), not both",
            ));
        }
        retain_schema_attributes(&mut field.attrs);
        if !range.is_empty() {
            field
                .attrs
                .push(parse_quote!(#[schemars(range(#(#range),*))]));
        }
        if !length.is_empty() {
            field
                .attrs
                .push(parse_quote!(#[schemars(length(#(#length),*))]));
        }
        rewrite.visit_type_mut(&mut field.ty);
    }
    Ok(())
}

fn numeric_literal(value: &Expr) -> syn::Result<f64> {
    let inner = match value {
        Expr::Unary(value) if matches!(value.op, syn::UnOp::Neg(_)) => value.expr.as_ref(),
        _ => value,
    };
    let parsed = match inner {
        Expr::Lit(value) => match &value.lit {
            Lit::Int(value) => value.base10_parse::<f64>().ok(),
            Lit::Float(value) => value.base10_parse::<f64>().ok(),
            _ => None,
        },
        _ => None,
    };
    parsed
        .filter(|value| value.is_finite())
        .map(|number| {
            if matches!(value, Expr::Unary(_)) {
                -number
            } else {
                number
            }
        })
        .ok_or_else(|| {
            syn::Error::new_spanned(value, "wire min/max require finite numeric literals")
        })
}
fn numeric_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if [
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64",
    ]
    .iter()
    .any(|name| segment.ident == name)
    {
        return true;
    }
    if segment.ident == "Option"
        && let PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(GenericArgument::Type(ty)) = args.args.first()
    {
        return numeric_type(ty);
    }
    false
}

fn length_type(ty: &Type) -> bool {
    match ty {
        Type::Array(_) | Type::Slice(_) => true,
        Type::Reference(reference) => length_type(&reference.elem),
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return false;
            };
            if [
                "String", "str", "Vec", "VecDeque", "HashMap", "BTreeMap", "HashSet", "BTreeSet",
            ]
            .iter()
            .any(|name| segment.ident == name)
            {
                return true;
            }
            if segment.ident == "Option"
                && let PathArguments::AngleBracketed(args) = &segment.arguments
                && let Some(GenericArgument::Type(ty)) = args.args.first()
            {
                return length_type(ty);
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse::Parser;

    fn procedure_error(options: Tokens, function: Tokens) -> String {
        let options = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated
            .parse2(options)
            .unwrap();
        expand_procedure(
            options.into_iter().collect(),
            syn::parse2(function).unwrap(),
        )
        .unwrap_err()
        .to_string()
    }

    #[test]
    fn procedure_options_have_deterministic_diagnostics() {
        let valid = quote!(
            async fn split(ctx: Ctx, input: u32) -> Result<u32, ApiError> {
                Ok(input)
            }
        );
        for (options, message) in [
            (
                quote!(name = "a", method = "POST"),
                "procedure requires path",
            ),
            (
                quote!(name = "a", method = "POST", path = "/", other = "x"),
                "unknown procedure option",
            ),
            (
                quote!(name = "a", name = "b", method = "POST", path = "/"),
                "duplicate procedure option",
            ),
            (
                quote!(name = 1, method = "POST", path = "/"),
                "procedure options must be string literals",
            ),
            (
                quote!(name = "has spaces", method = "POST", path = "/"),
                "procedure name must contain ASCII",
            ),
            (
                quote!(name = "a", method = "post", path = "/"),
                "unsupported procedure method",
            ),
            (
                quote!(name = "a", method = "POST", path = "/{id}"),
                "procedure path must be a static absolute path",
            ),
        ] {
            assert!(procedure_error(options, valid.clone()).starts_with(message));
        }
    }

    #[test]
    fn procedure_signatures_have_deterministic_diagnostics() {
        let options = quote!(name = "a", method = "POST", path = "/");
        for (function, message) in [
            (
                quote!(
                    fn split(ctx: Ctx, input: u32) -> Result<u32, ApiError> {
                        Ok(input)
                    }
                ),
                "procedure handler must be async",
            ),
            (
                quote!(
                    async fn split<T>(ctx: Ctx, input: T) -> Result<T, ApiError> {
                        Ok(input)
                    }
                ),
                "procedure handlers cannot be generic",
            ),
            (
                quote!(
                    async fn split(input: u32) -> Result<u32, ApiError> {
                        Ok(input)
                    }
                ),
                "procedure requires exactly two arguments",
            ),
            (
                quote!(
                    async fn split(ctx: &Ctx, input: u32) -> Result<u32, ApiError> {
                        Ok(input)
                    }
                ),
                "first procedure argument must be Ctx by value",
            ),
            (
                quote!(
                    async fn split(ctx: Ctx, input: &u32) -> Result<u32, ApiError> {
                        Ok(*input)
                    }
                ),
                "procedure input must be an owned value",
            ),
            (
                quote!(
                    async fn split(ctx: Ctx, input: u32) -> u32 {
                        input
                    }
                ),
                "procedure handler must return Result",
            ),
            (
                quote!(
                    async fn split(ctx: Ctx, input: u32) -> Result<u32, String> {
                        Ok(input)
                    }
                ),
                "procedure error type must be ApiError",
            ),
        ] {
            assert!(procedure_error(options.clone(), function).starts_with(message));
        }
    }

    #[test]
    fn unsupported_wire_shapes_and_attributes_have_diagnostics() {
        for (input, message) in [
            (
                quote!(union Invalid { value:u32 }),
                "WireType does not support unions",
            ),
            (
                quote!(
                    #[wire(min = 1)]
                    struct Invalid {
                        value: u32,
                    }
                ),
                "wire bounds belong on fields",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min = 1)]
                        value: String,
                    }
                ),
                "wire min/max require a numeric primitive",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min=f64::NAN)]
                        value: f64,
                    }
                ),
                "wire min/max require finite numeric literals",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min = 1e999)]
                        value: f64,
                    }
                ),
                "wire min/max require finite numeric literals",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min = 2, max = 1)]
                        value: u32,
                    }
                ),
                "wire min cannot exceed max",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min = 1, min = 2)]
                        value: u32,
                    }
                ),
                "duplicate wire bound",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(foo = 1)]
                        value: u32,
                    }
                ),
                "unsupported wire bound",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min_length = 1)]
                        value: u32,
                    }
                ),
                "wire length bounds require String",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min_length=-1)]
                        value: String,
                    }
                ),
                "wire length bounds require nonnegative integer literals",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min_length = 3, max_length = 1)]
                        value: String,
                    }
                ),
                "wire min_length cannot exceed max_length",
            ),
            (
                quote!(
                    struct Invalid {
                        #[serde(with = "codec")]
                        value: u32,
                    }
                ),
                "custom serde representations require an explicit",
            ),
            (
                quote!(
                    #[serde(default = "custom")]
                    struct Invalid {
                        value: u32,
                    }
                ),
                "custom container defaults are unsupported",
            ),
            (
                quote!(
                    #[derive(JsonSchema)]
                    struct Invalid {
                        value: u32,
                    }
                ),
                "WireType already implements JsonSchema",
            ),
            (
                quote!(
                    struct Invalid {
                        #[wire(min = 0)]
                        #[schemars(range(min = 0))]
                        value: u32,
                    }
                ),
                "use wire bounds or schemars(range), not both",
            ),
        ] {
            let actual = expand_wire(syn::parse2(input).unwrap())
                .unwrap_err()
                .to_string();
            assert!(
                actual.starts_with(message),
                "expected {message}, got {actual}"
            );
        }
    }
}
