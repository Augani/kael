use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use super::{get_simple_attribute_field, kael_crate_path};

pub fn derive_visual_context(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

    let window_variable = match get_simple_attribute_field(&ast, "window") {
        Ok(Some(field)) => field,
        Ok(None) => return quote! {
                compile_error!("Derive must have a #[window] attribute to detect the &mut Window field");
            }.into(),
        Err(error) => return error.into_compile_error().into(),
    };

    let app_variable =
        match get_simple_attribute_field(&ast, "app") {
            Ok(Some(field)) => field,
            Ok(None) => return quote! {
                compile_error!("Derive must have a #[app] attribute to detect the &mut App field");
            }
            .into(),
            Err(error) => return error.into_compile_error().into(),
        };
    let kael = match kael_crate_path() {
        Ok(path) => path,
        Err(error) => return error.into_compile_error().into(),
    };

    let type_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();

    let r#gen = quote! {
        #[automatically_derived]
        impl #impl_generics #kael::VisualContext for #type_name #type_generics
        #where_clause
        {
            fn window_handle(&self) -> #kael::AnyWindowHandle {
                self.#window_variable.window_handle()
            }

            fn update_window_entity<T: 'static, R>(
                &mut self,
                entity: &#kael::Entity<T>,
                update: impl FnOnce(&mut T, &mut #kael::Window, &mut #kael::Context<T>) -> R,
            ) -> Self::Result<R> {
                #kael::AppContext::update_entity(self.#app_variable, entity, |entity, cx| update(entity, self.#window_variable, cx))
            }

            fn new_window_entity<T: 'static>(
                &mut self,
                build_entity: impl FnOnce(&mut #kael::Window, &mut #kael::Context<'_, T>) -> T,
            ) -> Self::Result<#kael::Entity<T>> {
                #kael::AppContext::new(self.#app_variable, |cx| build_entity(self.#window_variable, cx))
            }

            fn replace_root_view<V>(
                &mut self,
                build_view: impl FnOnce(&mut #kael::Window, &mut #kael::Context<V>) -> V,
            ) -> Self::Result<#kael::Entity<V>>
            where
                V: 'static + #kael::Render,
            {
                self.#window_variable.replace_root(self.#app_variable, build_view)
            }

            fn focus<V>(&mut self, entity: &#kael::Entity<V>) -> Self::Result<()>
            where
                V: #kael::Focusable,
            {
                let focus_handle = #kael::Focusable::focus_handle(entity, self.#app_variable);
                self.#window_variable.focus(&focus_handle)
            }
        }
    };

    r#gen.into()
}
