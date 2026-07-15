use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

pub fn derive_render(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    let type_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();
    let kael = match crate::kael_crate_path() {
        Ok(path) => path,
        Err(error) => return error.into_compile_error().into(),
    };

    let r#gen = quote! {
        impl #impl_generics #kael::Render for #type_name #type_generics
        #where_clause
        {
            fn render(&mut self, _window: &mut #kael::Window, _cx: &mut #kael::Context<Self>) -> impl #kael::Element {
                #kael::Empty
            }
        }
    };

    r#gen.into()
}
