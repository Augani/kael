use crate::register_action::generate_register_action;
use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{Data, DeriveInput, LitStr, Token, parse::ParseStream};

pub(crate) fn derive_action(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "Action cannot be derived for generic types because actions are registered globally",
        )
        .into_compile_error()
        .into();
    }
    if matches!(input.data, Data::Union(_)) {
        return syn::Error::new_spanned(&input.ident, "Action cannot be derived for unions")
            .into_compile_error()
            .into();
    }
    let kael = match crate::kael_crate_path() {
        Ok(path) => path,
        Err(error) => return error.into_compile_error().into(),
    };

    let struct_name = &input.ident;
    let mut name_argument = None;
    let mut deprecated_aliases = Vec::new();
    let mut deprecated_aliases_set = false;
    let mut no_json = false;
    let mut no_register = false;
    let mut namespace = None;
    let mut deprecated = None;
    let mut doc_str: Option<String> = None;

    /*
    *
    * #[action()]
    * Struct Foo {
    *  bar: bool // is bar considered an attribute
    }
    */
    for attr in &input.attrs {
        if attr.path().is_ident("action") {
            if let Err(error) = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    if name_argument.is_some() {
                        return Err(meta.error("'name' argument specified multiple times"));
                    }
                    meta.input.parse::<Token![=]>()?;
                    let lit: LitStr = meta.input.parse()?;
                    name_argument = Some(lit);
                } else if meta.path.is_ident("namespace") {
                    if namespace.is_some() {
                        return Err(meta.error("'namespace' argument specified multiple times"));
                    }
                    meta.input.parse::<Token![=]>()?;
                    let ident: Ident = meta.input.parse()?;
                    namespace = Some(ident.to_string());
                } else if meta.path.is_ident("no_json") {
                    if no_json {
                        return Err(meta.error("'no_json' argument specified multiple times"));
                    }
                    no_json = true;
                } else if meta.path.is_ident("no_register") {
                    if no_register {
                        return Err(meta.error("'no_register' argument specified multiple times"));
                    }
                    no_register = true;
                } else if meta.path.is_ident("deprecated_aliases") {
                    if deprecated_aliases_set {
                        return Err(
                            meta.error("'deprecated_aliases' argument specified multiple times")
                        );
                    }
                    deprecated_aliases_set = true;
                    meta.input.parse::<Token![=]>()?;
                    // Parse array of string literals
                    let content;
                    syn::bracketed!(content in meta.input);
                    let aliases = content.parse_terminated(
                        |input: ParseStream| input.parse::<LitStr>(),
                        Token![,],
                    )?;
                    deprecated_aliases.extend(aliases);
                } else if meta.path.is_ident("deprecated") {
                    if deprecated.is_some() {
                        return Err(meta.error("'deprecated' argument specified multiple times"));
                    }
                    meta.input.parse::<Token![=]>()?;
                    let lit: LitStr = meta.input.parse()?;
                    deprecated = Some(lit.value());
                } else {
                    return Err(meta.error(format!(
                        "'{:?}' argument not recognized, expected \
                        'name', 'namespace', 'no_json', 'no_register', 'deprecated_aliases', or 'deprecated'",
                        meta.path
                    )));
                }
                Ok(())
            }) {
                return error.into_compile_error().into();
            }
        } else if attr.path().is_ident("doc") {
            use syn::{Expr::Lit, ExprLit, Lit::Str, Meta, MetaNameValue};
            if let Meta::NameValue(MetaNameValue {
                value:
                    Lit(ExprLit {
                        lit: Str(ref lit_str),
                        ..
                    }),
                ..
            }) = attr.meta
            {
                let doc = lit_str.value();
                let doc_str = doc_str.get_or_insert_default();
                doc_str.push_str(doc.trim());
                doc_str.push('\n');
            }
        }
    }

    let name = name_argument
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_else(|| struct_name.to_string());
    if let Err(message) = validate_action_name(&name, false) {
        let span = name_argument
            .as_ref()
            .map_or_else(|| struct_name.span(), LitStr::span);
        return syn::Error::new(span, message).into_compile_error().into();
    }

    let full_name = if let Some(namespace) = namespace {
        format!("{namespace}::{name}")
    } else {
        name
    };
    if let Err(message) = validate_action_name(&full_name, true) {
        return syn::Error::new_spanned(struct_name, message)
            .into_compile_error()
            .into();
    }

    for (index, alias) in deprecated_aliases.iter().enumerate() {
        let value = alias.value();
        if let Err(message) = validate_action_name(&value, true) {
            return syn::Error::new_spanned(alias, format!("invalid deprecated alias: {message}"))
                .into_compile_error()
                .into();
        }
        if value == full_name {
            return syn::Error::new_spanned(
                alias,
                "a deprecated alias must differ from the action's canonical name",
            )
            .into_compile_error()
            .into();
        }
        if deprecated_aliases[..index]
            .iter()
            .any(|previous| previous.value() == value)
        {
            return syn::Error::new_spanned(alias, "deprecated action aliases must be unique")
                .into_compile_error()
                .into();
        }
    }

    let is_unit_struct = matches!(&input.data, Data::Struct(data) if data.fields.is_empty());

    let build_fn_body = if no_json {
        let error_msg = format!("{} cannot be built from JSON", full_name);
        quote! { Err(#kael::private::anyhow::anyhow!(#error_msg)) }
    } else if is_unit_struct {
        quote! { Ok(Box::new(Self)) }
    } else {
        quote! { Ok(Box::new(#kael::private::serde_json::from_value::<Self>(_value)?)) }
    };

    let json_schema_fn_body = if no_json || is_unit_struct {
        quote! { None }
    } else {
        quote! { Some(<Self as #kael::private::schemars::JsonSchema>::json_schema(_generator)) }
    };

    let deprecated_aliases_fn_body = if deprecated_aliases.is_empty() {
        quote! { &[] }
    } else {
        let aliases = deprecated_aliases.iter();
        quote! { &[#(#aliases),*] }
    };

    let deprecation_fn_body = if let Some(message) = deprecated {
        quote! { Some(#message) }
    } else {
        quote! { None }
    };

    let documentation_fn_body = if let Some(doc) = doc_str {
        let doc = doc.trim();
        quote! { Some(#doc) }
    } else {
        quote! { None }
    };

    let registration = if no_register {
        quote! {}
    } else {
        generate_register_action(struct_name, &kael)
    };

    TokenStream::from(quote! {
        #registration

        impl #kael::Action for #struct_name {
            fn name(&self) -> &'static str {
                #full_name
            }

            fn name_for_type() -> &'static str
            where
                Self: Sized
            {
                #full_name
            }

            fn partial_eq(&self, action: &dyn #kael::Action) -> bool {
                action
                    .as_any()
                    .downcast_ref::<Self>()
                    .map_or(false, |a| self == a)
            }

            fn boxed_clone(&self) -> Box<dyn #kael::Action> {
                Box::new(self.clone())
            }

            fn build(_value: #kael::private::serde_json::Value) -> #kael::Result<Box<dyn #kael::Action>> {
                #build_fn_body
            }

            fn action_json_schema(
                _generator: &mut #kael::private::schemars::SchemaGenerator,
            ) -> Option<#kael::private::schemars::Schema> {
                #json_schema_fn_body
            }

            fn deprecated_aliases() -> &'static [&'static str] {
                #deprecated_aliases_fn_body
            }

            fn deprecation_message() -> Option<&'static str> {
                #deprecation_fn_body
            }

            fn documentation() -> Option<&'static str> {
                #documentation_fn_body
            }
        }
    })
}

const MAX_ACTION_NAME_BYTES: usize = 256;

fn validate_action_name(name: &str, allow_namespace: bool) -> Result<(), String> {
    if name.is_empty() {
        return Err("action name must not be empty".to_owned());
    }
    if name.len() > MAX_ACTION_NAME_BYTES {
        return Err(format!(
            "action name must not exceed {MAX_ACTION_NAME_BYTES} bytes"
        ));
    }
    if name.trim() != name {
        return Err("action name must not have leading or trailing whitespace".to_owned());
    }
    if name.chars().any(char::is_control) {
        return Err("action name must not contain control characters".to_owned());
    }
    if !allow_namespace && name.contains("::") {
        return Err(format!(
            "in #[action] attribute: `name = \"{name}\"` must not contain `::`; specify `namespace` instead"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_action_name;

    #[test]
    fn action_names_reject_ambiguous_protocol_values() {
        for name in ["", " Action", "Action ", "Action\nName", "scope::Action"] {
            assert!(validate_action_name(name, false).is_err(), "{name:?}");
        }
        assert!(validate_action_name(&"a".repeat(257), false).is_err());
    }

    #[test]
    fn deprecated_aliases_may_include_namespaces() {
        assert!(validate_action_name("old_scope::Action", true).is_ok());
    }
}
