#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::{
    DeriveInput, Field, FieldsNamed, PredicateType, TraitBound, Type, TypeParamBound,
    WherePredicate, parse_macro_input, parse_quote,
};

#[proc_macro_derive(Refineable, attributes(refineable))]
/// Generates a partial-update type and `Refineable` implementation for a named struct.
pub fn derive_refineable(input: TokenStream) -> TokenStream {
    derive_refineable_impl(parse_macro_input!(input))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn derive_refineable_impl(input: DeriveInput) -> syn::Result<TokenStream2> {
    let DeriveInput {
        ident,
        data,
        generics,
        attrs,
        ..
    } = input;

    let mut impl_debug_on_refinement = false;
    let mut derives_serialize = false;
    let mut derives_deserialize = false;
    let mut refinement_traits_to_derive = vec![];

    for refineable_attr in attrs
        .iter()
        .filter(|attr| attr.path().is_ident("refineable"))
    {
        refineable_attr.parse_nested_meta(|meta| {
            let trait_name = meta.path.segments.last().map(|segment| &segment.ident);
            if trait_name.is_some_and(|name| name == "Debug") {
                impl_debug_on_refinement = true;
            } else if trait_name.is_some_and(|name| name == "Clone") {
                // Every generated refinement is already Clone.
            } else {
                if trait_name.is_some_and(|name| name == "Serialize") {
                    derives_serialize = true;
                }
                if trait_name.is_some_and(|name| name == "Deserialize") {
                    derives_deserialize = true;
                }
                if !refinement_traits_to_derive.contains(&meta.path) {
                    refinement_traits_to_derive.push(meta.path);
                }
            }
            Ok(())
        })?;
    }

    let refinement_ident = format_ident!("{}Refinement", ident);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(FieldsNamed { named, .. }),
            ..
        }) => named.into_iter().collect::<Vec<Field>>(),
        _ => {
            return Err(syn::Error::new_spanned(
                ident,
                "Refineable can only be derived for structs with named fields",
            ));
        }
    };

    for field in &fields {
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("refineable"))
        {
            if !matches!(attr.meta, syn::Meta::Path(_)) {
                return Err(syn::Error::new_spanned(
                    attr,
                    "refineable fields use the marker form `#[refineable]`",
                ));
            }
        }
    }

    let refineable_crate = refineable_crate_path()?;
    let refineable_is_empty_path = syn::LitStr::new(
        &format!("{}::IsEmpty::is_empty", refineable_crate.to_token_stream()),
        proc_macro2::Span::call_site(),
    );

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_visibilities: Vec<_> = fields.iter().map(|f| &f.vis).collect();
    let wrapped_types: Vec<_> = fields
        .iter()
        .map(|field| get_wrapper_type(field, &field.ty))
        .collect::<syn::Result<_>>()?;

    let field_attributes: Vec<TokenStream2> = fields
        .iter()
        .map(|f| {
            if is_refineable_field(f) {
                match (derives_serialize, derives_deserialize) {
                    (true, true) => {
                        quote! { #[serde(default, skip_serializing_if = #refineable_is_empty_path)] }
                    }
                    (true, false) => {
                        quote! { #[serde(skip_serializing_if = #refineable_is_empty_path)] }
                    }
                    (false, true) => quote! { #[serde(default)] },
                    (false, false) => quote! {},
                }
            } else if derives_serialize {
                quote! { #[serde(skip_serializing_if = "::std::option::Option::is_none")] }
            } else {
                quote! {}
            }
        })
        .collect();

    // Generated refinements clone every wrapped field. Non-nested values are
    // also compared, and regular values need a default for `From<Refinement>`.
    let mut type_param_bounds = Vec::new();
    for (field, wrapped_type) in fields.iter().zip(&wrapped_types) {
        type_param_bounds.push(type_bound(wrapped_type.clone(), parse_quote!(Clone)));
        if !is_refineable_field(field) {
            type_param_bounds.push(type_bound(field.ty.clone(), parse_quote!(PartialEq)));
            if !is_optional_field(field) {
                type_param_bounds.push(type_bound(field.ty.clone(), parse_quote!(Default)));
            }
        }
    }

    // Append to where_clause or create a new one if it doesn't exist
    let where_clause = match (where_clause.cloned(), type_param_bounds.is_empty()) {
        (where_clause, true) => where_clause,
        (Some(mut where_clause), false) => {
            where_clause.predicates.extend(type_param_bounds);
            Some(where_clause)
        }
        (None, false) => Some(parse_quote!(where #(#type_param_bounds),*)),
    };

    let refineable_refine_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);
            let is_optional = is_optional_field(field);

            if is_refineable {
                quote! {
                    #refineable_crate::Refineable::refine(
                        &mut self.#name,
                        &refinement.#name,
                    );
                }
            } else if is_optional {
                quote! {
                    if let Some(value) = &refinement.#name {
                        self.#name = Some(value.clone());
                    }
                }
            } else {
                quote! {
                    if let Some(value) = &refinement.#name {
                        self.#name = value.clone();
                    }
                }
            }
        })
        .collect();

    let refineable_refined_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);
            let is_optional = is_optional_field(field);

            if is_refineable {
                quote! {
                    self.#name = #refineable_crate::Refineable::refined(
                        self.#name,
                        refinement.#name,
                    );
                }
            } else if is_optional {
                quote! {
                    if let Some(value) = refinement.#name {
                        self.#name = Some(value);
                    }
                }
            } else {
                quote! {
                    if let Some(value) = refinement.#name {
                        self.#name = value;
                    }
                }
            }
        })
        .collect();

    let refinement_refine_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);

            if is_refineable {
                quote! {
                    #refineable_crate::Refineable::refine(
                        &mut self.#name,
                        &refinement.#name,
                    );
                }
            } else {
                quote! {
                    if let Some(value) = &refinement.#name {
                        self.#name = Some(value.clone());
                    }
                }
            }
        })
        .collect();

    let refinement_refined_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);

            if is_refineable {
                quote! {
                    self.#name = #refineable_crate::Refineable::refined(
                        self.#name,
                        refinement.#name,
                    );
                }
            } else {
                quote! {
                    if let Some(value) = refinement.#name {
                        self.#name = Some(value);
                    }
                }
            }
        })
        .collect();

    let from_refinement_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);
            let is_optional = is_optional_field(field);

            if is_refineable {
                quote! {
                    #name: value.#name.into(),
                }
            } else if is_optional {
                quote! {
                    #name: value.#name.map(|v| v.into()),
                }
            } else {
                quote! {
                    #name: value.#name.map(|v| v.into()).unwrap_or_default(),
                }
            }
        })
        .collect();

    let debug_impl = if impl_debug_on_refinement {
        let refinement_field_debugs: Vec<TokenStream2> = fields
            .iter()
            .map(|field| {
                let name = &field.ident;
                quote! {
                    if self.#name.is_some() {
                        debug_struct.field(stringify!(#name), &self.#name);
                    } else {
                        all_some = false;
                    }
                }
            })
            .collect();

        quote! {
            impl #impl_generics std::fmt::Debug for #refinement_ident #ty_generics
                #where_clause
            {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut debug_struct = f.debug_struct(stringify!(#refinement_ident));
                    let mut all_some = true;
                    #( #refinement_field_debugs )*
                    if all_some {
                        debug_struct.finish()
                    } else {
                        debug_struct.finish_non_exhaustive()
                    }
                }
            }
        }
    } else {
        quote! {}
    };

    let refinement_is_empty_conditions: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;

            if is_refineable_field(field) {
                quote! { #refineable_crate::IsEmpty::is_empty(&self.#name) }
            } else {
                quote! { self.#name.is_none() }
            }
        })
        .collect();

    let refineable_is_superset_conditions: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);
            let is_optional = is_optional_field(field);

            if is_refineable {
                quote! {
                    if !#refineable_crate::Refineable::is_superset_of(
                        &self.#name,
                        &refinement.#name,
                    ) {
                        return false;
                    }
                }
            } else if is_optional {
                quote! {
                    if refinement.#name.is_some() && &self.#name != &refinement.#name {
                        return false;
                    }
                }
            } else {
                quote! {
                    if let Some(refinement_value) = &refinement.#name {
                        if &self.#name != refinement_value {
                            return false;
                        }
                    }
                }
            }
        })
        .collect();

    let refinement_is_superset_conditions: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);

            if is_refineable {
                quote! {
                    if !#refineable_crate::Refineable::is_superset_of(
                        &self.#name,
                        &refinement.#name,
                    ) {
                        return false;
                    }
                }
            } else {
                quote! {
                    if refinement.#name.is_some() && &self.#name != &refinement.#name {
                        return false;
                    }
                }
            }
        })
        .collect();

    let refineable_subtract_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);
            let is_optional = is_optional_field(field);

            if is_refineable {
                quote! {
                    #name: #refineable_crate::Refineable::subtract(
                        &self.#name,
                        &refinement.#name,
                    ),
                }
            } else if is_optional {
                quote! {
                    #name: if &self.#name == &refinement.#name {
                        None
                    } else {
                        self.#name.clone()
                    },
                }
            } else {
                quote! {
                    #name: if let Some(refinement_value) = &refinement.#name {
                        if &self.#name == refinement_value {
                            None
                        } else {
                            Some(self.#name.clone())
                        }
                    } else {
                        Some(self.#name.clone())
                    },
                }
            }
        })
        .collect();

    let refinement_subtract_assignments: Vec<TokenStream2> = fields
        .iter()
        .map(|field| {
            let name = &field.ident;
            let is_refineable = is_refineable_field(field);

            if is_refineable {
                quote! {
                    #name: #refineable_crate::Refineable::subtract(
                        &self.#name,
                        &refinement.#name,
                    ),
                }
            } else {
                quote! {
                    #name: if &self.#name == &refinement.#name {
                        None
                    } else {
                        self.#name.clone()
                    },
                }
            }
        })
        .collect();

    let mut derive_stream = quote! {};
    for trait_to_derive in refinement_traits_to_derive {
        derive_stream.extend(quote! { #[derive(#trait_to_derive)] })
    }

    let r#gen = quote! {
        /// A refinable version of [`#ident`], see that documentation for details.
        #[derive(Clone)]
        #derive_stream
        pub struct #refinement_ident #impl_generics
            #where_clause
        {
            #(
                #[allow(missing_docs)]
                #field_attributes
                #field_visibilities #field_names: #wrapped_types
            ),*
        }

        impl #impl_generics #refineable_crate::Refineable for #ident #ty_generics
            #where_clause
        {
            type Refinement = #refinement_ident #ty_generics;

            fn refine(&mut self, refinement: &Self::Refinement) {
                #( #refineable_refine_assignments )*
            }

            fn refined(mut self, refinement: Self::Refinement) -> Self {
                #( #refineable_refined_assignments )*
                self
            }

            fn is_superset_of(&self, refinement: &Self::Refinement) -> bool
            {
                #( #refineable_is_superset_conditions )*
                true
            }

            fn subtract(&self, refinement: &Self::Refinement) -> Self::Refinement
            {
                #refinement_ident {
                    #( #refineable_subtract_assignments )*
                }
            }
        }

        impl #impl_generics #refineable_crate::Refineable for #refinement_ident #ty_generics
            #where_clause
        {
            type Refinement = #refinement_ident #ty_generics;

            fn refine(&mut self, refinement: &Self::Refinement) {
                #( #refinement_refine_assignments )*
            }

            fn refined(mut self, refinement: Self::Refinement) -> Self {
                #( #refinement_refined_assignments )*
                self
            }

            fn is_superset_of(&self, refinement: &Self::Refinement) -> bool
            {
                #( #refinement_is_superset_conditions )*
                true
            }

            fn subtract(&self, refinement: &Self::Refinement) -> Self::Refinement
            {
                #refinement_ident {
                    #( #refinement_subtract_assignments )*
                }
            }
        }

        impl #impl_generics #refineable_crate::IsEmpty for #refinement_ident #ty_generics
            #where_clause
        {
            fn is_empty(&self) -> bool {
                true #( && #refinement_is_empty_conditions )*
            }
        }

        impl #impl_generics From<#refinement_ident #ty_generics> for #ident #ty_generics
            #where_clause
        {
            fn from(value: #refinement_ident #ty_generics) -> Self {
                Self {
                    #( #from_refinement_assignments )*
                }
            }
        }

        impl #impl_generics ::core::default::Default for #refinement_ident #ty_generics
            #where_clause
        {
            fn default() -> Self {
                #refinement_ident {
                    #( #field_names: Default::default() ),*
                }
            }
        }

        impl #impl_generics #refinement_ident #ty_generics
            #where_clause
        {
            /// Returns `true` if at least one field has a refinement value.
            pub fn is_some(&self) -> bool {
                #(
                    if self.#field_names.is_some() {
                        return true;
                    }
                )*
                false
            }
        }

        #debug_impl
    };
    Ok(r#gen)
}

fn is_refineable_field(f: &Field) -> bool {
    f.attrs
        .iter()
        .any(|attr| attr.path().is_ident("refineable"))
}

fn is_optional_field(f: &Field) -> bool {
    if let Type::Path(typepath) = &f.ty
        && typepath.qself.is_none()
    {
        let segments = &typepath.path.segments;
        return (segments.len() == 1 && segments[0].ident == "Option")
            || (segments.len() == 3
                && (segments[0].ident == "std" || segments[0].ident == "core")
                && segments[1].ident == "option"
                && segments[2].ident == "Option");
    }
    false
}

fn get_wrapper_type(field: &Field, ty: &Type) -> syn::Result<syn::Type> {
    if is_refineable_field(field) {
        if is_optional_field(field) {
            return Err(syn::Error::new_spanned(
                ty,
                "an optional field cannot use `#[refineable]`; refine the inner value separately",
            ));
        }
        let Type::Path(mut type_path) = ty.clone() else {
            return Err(syn::Error::new_spanned(
                ty,
                "a refineable field must use a named struct type",
            ));
        };
        if type_path.qself.is_some() {
            return Err(syn::Error::new_spanned(
                ty,
                "a refineable field cannot use a qualified associated type",
            ));
        }
        let Some(segment) = type_path.path.segments.last_mut() else {
            return Err(syn::Error::new_spanned(
                ty,
                "a refineable field must use a named struct type",
            ));
        };
        segment.ident = format_ident!("{}Refinement", segment.ident);
        Ok(Type::Path(type_path))
    } else if is_optional_field(field) {
        Ok(ty.clone())
    } else {
        Ok(parse_quote!(Option<#ty>))
    }
}

fn type_bound(ty: Type, bound: syn::Path) -> WherePredicate {
    WherePredicate::Type(PredicateType {
        lifetimes: None,
        bounded_ty: ty,
        colon_token: Default::default(),
        bounds: {
            let mut bounds = syn::punctuated::Punctuated::new();
            bounds.push_value(TypeParamBound::Trait(TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: bound,
            }));
            bounds
        },
    })
}

fn refineable_crate_path() -> syn::Result<syn::Path> {
    match crate_name("kael_refineable").map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("could not locate the kael_refineable dependency: {error}"),
        )
    })? {
        // Examples, doctests, and integration targets belong to the
        // `kael_refineable` package, so `proc_macro_crate` reports `Itself`
        // even though their `crate` root is not the library. The runtime crate
        // exposes this stable self-alias for its own derives.
        FoundCrate::Itself => Ok(parse_quote!(::kael_refineable)),
        FoundCrate::Name(name) => {
            let ident = format_ident!("{}", name.replace('-', "_"));
            Ok(parse_quote!(::#ident))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_refinement_preserves_qualified_path_and_generics() {
        let field: Field = parse_quote! {
            #[refineable]
            child: crate::theme::Child<u8>
        };

        let wrapped = get_wrapper_type(&field, &field.ty).unwrap();

        assert_eq!(wrapped, parse_quote!(crate::theme::ChildRefinement<u8>));
    }

    #[test]
    fn nested_refinement_rejects_non_path_types() {
        let field: Field = parse_quote! {
            #[refineable]
            child: (u8, u8)
        };

        let error = get_wrapper_type(&field, &field.ty).unwrap_err();

        assert!(error.to_string().contains("named struct type"));
    }

    #[test]
    fn tuple_structs_receive_a_compile_diagnostic() {
        let input: DeriveInput = parse_quote! {
            struct Unsupported(u8);
        };

        let error = derive_refineable_impl(input).unwrap_err();

        assert!(error.to_string().contains("structs with named fields"));
    }

    #[test]
    fn qualified_standard_option_paths_are_recognized() {
        for field in [
            parse_quote!(value: Option<u8>),
            parse_quote!(value: std::option::Option<u8>),
            parse_quote!(value: ::core::option::Option<u8>),
        ] {
            assert!(is_optional_field(&field));
        }

        let custom: Field = parse_quote!(value: crate::option::Option<u8>);
        assert!(!is_optional_field(&custom));
    }

    #[test]
    fn optional_nested_refinements_receive_a_clear_diagnostic() {
        let field: Field = parse_quote! {
            #[refineable]
            child: Option<Child>
        };

        let error = get_wrapper_type(&field, &field.ty).unwrap_err();

        assert!(error.to_string().contains("optional field"));
    }
}
