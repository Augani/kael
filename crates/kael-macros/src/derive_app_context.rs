use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

use crate::{get_simple_attribute_field, kael_crate_path};

pub fn derive_app_context(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

    let app_variable = match get_simple_attribute_field(&ast, "app") {
        Ok(Some(field)) => field,
        Ok(None) => {
            return quote! {
                compile_error!("Derive must have an #[app] attribute to detect the &mut App field");
            }
            .into();
        }
        Err(error) => return error.into_compile_error().into(),
    };
    let kael = match kael_crate_path() {
        Ok(path) => path,
        Err(error) => return error.into_compile_error().into(),
    };

    let type_name = &ast.ident;
    let (impl_generics, type_generics, where_clause) = ast.generics.split_for_impl();

    let r#gen = quote! {
        impl #impl_generics #kael::AppContext for #type_name #type_generics
        #where_clause
        {
            type Result<T> = T;

            fn new<T: 'static>(
                &mut self,
                build_entity: impl FnOnce(&mut #kael::Context<'_, T>) -> T,
            ) -> Self::Result<#kael::Entity<T>> {
                self.#app_variable.new(build_entity)
            }

            fn reserve_entity<T: 'static>(&mut self) -> Self::Result<#kael::Reservation<T>> {
                self.#app_variable.reserve_entity()
            }

            fn insert_entity<T: 'static>(
                &mut self,
                reservation: #kael::Reservation<T>,
                build_entity: impl FnOnce(&mut #kael::Context<'_, T>) -> T,
            ) -> Self::Result<#kael::Entity<T>> {
                self.#app_variable.insert_entity(reservation, build_entity)
            }

            fn update_entity<T, R>(
                &mut self,
                handle: &#kael::Entity<T>,
                update: impl FnOnce(&mut T, &mut #kael::Context<'_, T>) -> R,
            ) -> Self::Result<R>
            where
                T: 'static,
            {
                self.#app_variable.update_entity(handle, update)
            }

            fn as_mut<'y, 'z, T>(
                &'y mut self,
                handle: &'z #kael::Entity<T>,
            ) -> Self::Result<#kael::GpuiBorrow<'y, T>>
            where
                T: 'static,
            {
                self.#app_variable.as_mut(handle)
            }

            fn read_entity<T, R>(
                &self,
                handle: &#kael::Entity<T>,
                read: impl FnOnce(&T, &#kael::App) -> R,
            ) -> Self::Result<R>
            where
                T: 'static,
            {
                self.#app_variable.read_entity(handle, read)
            }

            fn update_window<T, F>(&mut self, window: #kael::AnyWindowHandle, f: F) -> #kael::Result<T>
            where
                F: FnOnce(#kael::AnyView, &mut #kael::Window, &mut #kael::App) -> T,
            {
                self.#app_variable.update_window(window, f)
            }

            fn read_window<T, R>(
                &self,
                window: &#kael::WindowHandle<T>,
                read: impl FnOnce(#kael::Entity<T>, &#kael::App) -> R,
            ) -> #kael::Result<R>
            where
                T: 'static,
            {
                self.#app_variable.read_window(window, read)
            }

            fn background_spawn<R>(&self, future: impl ::std::future::Future<Output = R> + Send + 'static) -> #kael::Task<R>
            where
                R: Send + 'static,
            {
                self.#app_variable.background_spawn(future)
            }

            fn read_global<G, R>(&self, callback: impl FnOnce(&G, &#kael::App) -> R) -> Self::Result<R>
            where
                G: #kael::Global,
            {
                self.#app_variable.read_global(callback)
            }
        }
    };

    r#gen.into()
}
