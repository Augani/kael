#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod derive_action;
mod derive_app_context;
mod derive_into_element;
mod derive_render;
mod derive_visual_context;
mod register_action;
mod styles;
mod test;

#[cfg(any(feature = "inspector", debug_assertions))]
mod derive_inspector_reflection;

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::format_ident;
use syn::{DeriveInput, Ident, Meta, parse_quote};

/// Derives Kael's `Action` protocol implementation for a concrete type.
///
/// See [`kael::Action`](https://docs.rs/kael/latest/kael/trait.Action.html) for
/// supported attributes and runtime behavior.
#[proc_macro_derive(Action, attributes(action))]
pub fn derive_action(input: TokenStream) -> TokenStream {
    derive_action::derive_action(input)
}

/// Registers an action with the Kael runtime when you manually implement
/// the `Action` trait. Typically you should use the `Action` derive macro or `actions!` macro
/// instead.
#[proc_macro]
pub fn register_action(ident: TokenStream) -> TokenStream {
    register_action::register_action(ident)
}

/// Derives `IntoElement` for a type that implements `RenderOnce`.
#[proc_macro_derive(IntoElement)]
pub fn derive_into_element(input: TokenStream) -> TokenStream {
    derive_into_element::derive_into_element(input)
}

#[proc_macro_derive(Render)]
#[doc(hidden)]
pub fn derive_render(input: TokenStream) -> TokenStream {
    derive_render::derive_render(input)
}

/// Derives `AppContext` for a type that holds a `&mut App`.
///
/// An argument-free `#[app]` attribute is required on exactly one named field.
///
/// Failure to add the attribute causes a compile error:
///
/// ```compile_fail
/// # #[macro_use] extern crate kael_macros;
/// # #[macro_use] extern crate kael;
/// #[derive(AppContext)]
/// struct MyContext<'a> {
///     app: &'a mut kael::App
/// }
/// ```
#[proc_macro_derive(AppContext, attributes(app))]
pub fn derive_app_context(input: TokenStream) -> TokenStream {
    derive_app_context::derive_app_context(input)
}

/// Derives `VisualContext` for an `AppContext` that also holds a `&mut Window`.
///
/// Argument-free `#[app]` and `#[window]` attributes identify the named fields
/// holding `&mut App` and `&mut Window`, respectively.
///
/// Failure to add both attributes causes a compile error:
///
/// ```compile_fail
/// # #[macro_use] extern crate kael_macros;
/// # #[macro_use] extern crate kael;
/// #[derive(VisualContext)]
/// struct MyContext<'a, 'b> {
///     #[app]
///     app: &'a mut kael::App,
///     window: &'b mut kael::Window
/// }
/// ```
///
/// ```compile_fail
/// # #[macro_use] extern crate kael_macros;
/// # #[macro_use] extern crate kael;
/// #[derive(VisualContext)]
/// struct MyContext<'a, 'b> {
///     app: &'a mut kael::App,
///     #[window]
///     window: &'b mut kael::Window
/// }
/// ```
#[proc_macro_derive(VisualContext, attributes(window, app))]
pub fn derive_visual_context(input: TokenStream) -> TokenStream {
    derive_visual_context::derive_visual_context(input)
}

/// Used by GPUI to generate the style helpers.
#[proc_macro]
#[doc(hidden)]
pub fn style_helpers(input: TokenStream) -> TokenStream {
    styles::style_helpers(input)
}

/// Generates methods for visibility styles.
#[proc_macro]
pub fn visibility_style_methods(input: TokenStream) -> TokenStream {
    styles::visibility_style_methods(input)
}

/// Generates methods for margin styles.
#[proc_macro]
pub fn margin_style_methods(input: TokenStream) -> TokenStream {
    styles::margin_style_methods(input)
}

/// Generates methods for padding styles.
#[proc_macro]
pub fn padding_style_methods(input: TokenStream) -> TokenStream {
    styles::padding_style_methods(input)
}

/// Generates methods for position styles.
#[proc_macro]
pub fn position_style_methods(input: TokenStream) -> TokenStream {
    styles::position_style_methods(input)
}

/// Generates methods for overflow styles.
#[proc_macro]
pub fn overflow_style_methods(input: TokenStream) -> TokenStream {
    styles::overflow_style_methods(input)
}

/// Generates methods for cursor styles.
#[proc_macro]
pub fn cursor_style_methods(input: TokenStream) -> TokenStream {
    styles::cursor_style_methods(input)
}

/// Generates methods for border styles.
#[proc_macro]
pub fn border_style_methods(input: TokenStream) -> TokenStream {
    styles::border_style_methods(input)
}

/// Generates methods for box shadow styles.
#[proc_macro]
pub fn box_shadow_style_methods(input: TokenStream) -> TokenStream {
    styles::box_shadow_style_methods(input)
}

/// `#[kael::test]` annotates test functions that run with Kael support.
///
/// It supports both synchronous and asynchronous tests, and can provide you with
/// as many `TestAppContext` instances as you need.
/// The output contains a `#[test]` annotation so this can be used with any existing
/// test harness (`cargo test` or `cargo-nextest`).
///
/// ```
/// # extern crate kael_renamed as kael;
/// # use kael::TestAppContext;
/// #[kael::test]
/// async fn test_foo(mut cx: &TestAppContext) { }
/// ```
///
/// In addition to passing a `TestAppContext`, you can also ask for a `StdRng` instance.
/// It is seeded with the `SEED` environment variable and is used internally by
/// the foreground and background executors to run tasks deterministically in tests.
/// Using the same `StdRng` for behavior in your test will allow you to exercise a wide
/// variety of scenarios and interleavings just by changing the seed.
///
/// # Arguments
///
/// - `#[kael::test]` with no arguments runs once with the seed `0` or `SEED` env var if set.
/// - `#[kael::test(seed = 10)]` runs once with the seed `10`.
/// - `#[kael::test(seeds(10, 20, 30))]` runs three times with seeds `10`, `20`, and `30`.
/// - `#[kael::test(iterations = 5)]` runs five times, providing as seed the values in the range `0..5`.
/// - `#[kael::test(retries = 3)]` runs up to four times if it fails to try and make it pass.
/// - `#[kael::test(on_failure = "crate::test::report_failure")]` will call the specified function after the
///   tests fail so that you can write out more detail about the failure.
///
/// You can combine `iterations = ...` with `seeds(...)`:
/// - `#[kael::test(iterations = 5, seed = 10)]` is equivalent to `#[kael::test(seeds(0, 1, 2, 3, 4, 10))]`.
/// - `#[kael::test(iterations = 5, seeds(10, 20, 30))]` is equivalent to `#[kael::test(seeds(0, 1, 2, 3, 4, 10, 20, 30))]`.
/// - `#[kael::test(seeds(10, 20, 30), iterations = 5)]` is equivalent to `#[kael::test(seeds(0, 1, 2, 3, 4, 10, 20, 30))]`.
///
/// # Environment Variables
///
/// - `SEED`: sets a seed for the first run
/// - `ITERATIONS`: forces the value of the `iterations` argument
#[proc_macro_attribute]
pub fn test(args: TokenStream, function: TokenStream) -> TokenStream {
    test::test(args, function)
}

/// When added to a trait, `#[derive_inspector_reflection]` generates a module which provides
/// enumeration and lookup by name of all methods that have the shape `fn method(self) -> Self`.
/// This is used by the inspector so that it can use the builder methods in `Styled` and
/// `StyledExt`.
///
/// The generated module will have the name `<snake_case_trait_name>_reflection` and contain the
/// following functions:
///
/// ```ignore
/// pub fn methods::<T: TheTrait + 'static>() -> Vec<kael::inspector_reflection::FunctionReflection<T>>;
///
/// pub fn find_method::<T: TheTrait + 'static>() -> Option<kael::inspector_reflection::FunctionReflection<T>>;
/// ```
///
/// The `invoke` method on `FunctionReflection` will run the method. `FunctionReflection` also
/// provides the method's documentation.
#[cfg(any(feature = "inspector", debug_assertions))]
#[proc_macro_attribute]
pub fn derive_inspector_reflection(_args: TokenStream, input: TokenStream) -> TokenStream {
    derive_inspector_reflection::derive_inspector_reflection(_args, input)
}

pub(crate) fn get_simple_attribute_field(
    ast: &DeriveInput,
    name: &'static str,
) -> syn::Result<Option<Ident>> {
    let syn::Data::Struct(data_struct) = &ast.data else {
        return Ok(None);
    };
    let mut matching_field = None;
    for field in &data_struct.fields {
        for attribute in field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident(name))
        {
            if !matches!(&attribute.meta, Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    attribute,
                    format!("#[{name}] does not accept arguments"),
                ));
            }
            if matching_field.is_some() {
                return Err(syn::Error::new_spanned(
                    attribute,
                    format!("only one field may be marked #[{name}]"),
                ));
            }
            matching_field = Some(field.ident.clone().ok_or_else(|| {
                syn::Error::new_spanned(
                    field,
                    format!("#[{name}] must be placed on a named struct field"),
                )
            })?);
        }
    }
    Ok(matching_field)
}

pub(crate) fn kael_crate_path() -> syn::Result<syn::Path> {
    match crate_name("kael").map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("could not locate the kael dependency: {error}"),
        )
    })? {
        // Examples and integration targets belong to the `kael` package, so
        // `proc_macro_crate` reports `Itself` even though their `crate` root is
        // not the library. The library exposes this alias for its own derives.
        FoundCrate::Itself => Ok(parse_quote!(::kael)),
        FoundCrate::Name(name) => {
            let ident = format_ident!("{}", name.replace('-', "_"));
            Ok(parse_quote!(::#ident))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::get_simple_attribute_field;

    #[test]
    fn simple_field_attributes_reject_arguments() {
        let input = syn::parse_quote! {
            struct Context<'a> {
                #[app(unexpected)]
                app: &'a mut u8,
            }
        };
        let error = get_simple_attribute_field(&input, "app").unwrap_err();
        assert_eq!(error.to_string(), "#[app] does not accept arguments");
    }

    #[test]
    fn simple_field_attributes_reject_duplicates_on_one_field() {
        let input = syn::parse_quote! {
            struct Context<'a> {
                #[app]
                #[app]
                app: &'a mut u8,
            }
        };
        let error = get_simple_attribute_field(&input, "app").unwrap_err();
        assert_eq!(error.to_string(), "only one field may be marked #[app]");
    }
}
