use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::{format_ident, quote};
use std::mem;
use syn::{
    self, Expr, ExprLit, FnArg, ItemFn, Lit, Meta, MetaList, Token, Type,
    ext::IdentExt as _,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
};

struct Args {
    seeds: Vec<u64>,
    max_retries: usize,
    max_iterations: usize,
    on_failure_fn_name: proc_macro2::TokenStream,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        let mut seeds = Vec::<u64>::new();
        let mut max_retries = 0;
        let mut max_iterations = 1;
        let mut on_failure_fn_name = quote!(None);
        let mut retries_set = false;
        let mut iterations_set = false;
        let mut on_failure_set = false;
        let mut seed_set = false;
        let mut seeds_set = false;

        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;

        for meta in metas {
            let ident = {
                let meta_path = match &meta {
                    Meta::NameValue(meta) => &meta.path,
                    Meta::List(list) => &list.path,
                    Meta::Path(path) => {
                        return Err(syn::Error::new(path.span(), "invalid path argument"));
                    }
                };
                let Some(ident) = meta_path.get_ident() else {
                    return Err(syn::Error::new(meta_path.span(), "unexpected path"));
                };
                ident.to_string()
            };

            match (&meta, ident.as_str()) {
                (Meta::NameValue(meta), "retries") => {
                    reject_duplicate(&mut retries_set, meta, "retries")?;
                    max_retries = parse_usize_from_expr(&meta.value)?
                }
                (Meta::NameValue(meta), "iterations") => {
                    reject_duplicate(&mut iterations_set, meta, "iterations")?;
                    max_iterations = parse_usize_from_expr(&meta.value)?;
                    if max_iterations == 0 {
                        return Err(syn::Error::new(
                            meta.value.span(),
                            "iterations must be greater than zero",
                        ));
                    }
                }
                (Meta::NameValue(meta), "on_failure") => {
                    reject_duplicate(&mut on_failure_set, meta, "on_failure")?;
                    let Expr::Lit(ExprLit {
                        lit: Lit::Str(name),
                        ..
                    }) = &meta.value
                    else {
                        return Err(syn::Error::new(
                            meta.value.span(),
                            "on_failure argument must be a string",
                        ));
                    };
                    let path = syn::parse_str::<syn::Path>(&name.value()).map_err(|error| {
                        syn::Error::new(name.span(), format!("invalid on_failure path: {error}"))
                    })?;
                    on_failure_fn_name = quote!(Some(#path));
                }
                (Meta::NameValue(meta), "seed") => {
                    reject_duplicate(&mut seed_set, meta, "seed")?;
                    seeds.push(parse_u64_from_expr(&meta.value)?);
                }
                (Meta::List(list), "seeds") => {
                    reject_duplicate(&mut seeds_set, list, "seeds")?;
                    seeds.extend(parse_u64_array(list)?);
                }
                (Meta::Path(_), _) => {
                    return Err(syn::Error::new(meta.span(), "invalid path argument"));
                }
                (_, _) => {
                    return Err(syn::Error::new(meta.span(), "invalid argument name"));
                }
            }
        }

        Ok(Args {
            seeds,
            max_retries,
            max_iterations,
            on_failure_fn_name,
        })
    }
}

fn reject_duplicate(already_set: &mut bool, meta: &impl Spanned, name: &str) -> syn::Result<()> {
    if *already_set {
        Err(syn::Error::new(
            meta.span(),
            format!("'{name}' specified multiple times"),
        ))
    } else {
        *already_set = true;
        Ok(())
    }
}

pub fn test(args: TokenStream, function: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(args as Args);
    let mut inner_fn = match syn::parse::<ItemFn>(function) {
        Ok(f) => f,
        Err(err) => return error_to_stream(err),
    };

    let inner_fn_attributes = mem::take(&mut inner_fn.attrs);
    let inner_fn_name = format_ident!("__{}", inner_fn.sig.ident.unraw());
    let outer_fn_name = mem::replace(&mut inner_fn.sig.ident, inner_fn_name.clone());
    let kael = match crate::kael_crate_path() {
        Ok(path) => path,
        Err(error) => return error.into_compile_error().into(),
    };

    let result = generate_test_function(
        args,
        inner_fn,
        inner_fn_attributes,
        inner_fn_name,
        outer_fn_name,
        &kael,
    );
    match result {
        Ok(tokens) => tokens,
        Err(tokens) => tokens,
    }
}

fn generate_test_function(
    args: Args,
    inner_fn: ItemFn,
    inner_fn_attributes: Vec<syn::Attribute>,
    inner_fn_name: Ident,
    outer_fn_name: Ident,
    kael: &syn::Path,
) -> Result<TokenStream, TokenStream> {
    let seeds = &args.seeds;
    let max_retries = args.max_retries;
    let num_iterations = args.max_iterations;
    let on_failure_fn_name = &args.on_failure_fn_name;
    let seeds = quote!( #(#seeds),* );

    let mut outer_fn: ItemFn = if inner_fn.sig.asyncness.is_some() {
        // Pass to the test function the number of app contexts that it needs,
        // based on its parameter list.
        let mut cx_vars = proc_macro2::TokenStream::new();
        let mut cx_teardowns = proc_macro2::TokenStream::new();
        let mut inner_fn_args = proc_macro2::TokenStream::new();
        for (ix, arg) in inner_fn.sig.inputs.iter().enumerate() {
            if let FnArg::Typed(arg) = arg {
                if let Type::Path(ty) = &*arg.ty {
                    let last_segment = ty.path.segments.last();
                    match last_segment.map(|s| s.ident.to_string()).as_deref() {
                        Some("StdRng") => {
                            inner_fn_args.extend(quote!(rand::SeedableRng::seed_from_u64(_seed),));
                            continue;
                        }
                        Some("BackgroundExecutor") => {
                            inner_fn_args.extend(quote!(#kael::BackgroundExecutor::new(
                                ::std::sync::Arc::new(dispatcher.clone()),
                            ),));
                            continue;
                        }
                        _ => {}
                    }
                } else if let Type::Reference(ty) = &*arg.ty
                    && let Type::Path(ty) = &*ty.elem
                {
                    let last_segment = ty.path.segments.last();
                    if let Some("TestAppContext") =
                        last_segment.map(|s| s.ident.to_string()).as_deref()
                    {
                        let cx_varname = format_ident!("cx_{}", ix);
                        cx_vars.extend(quote!(
                            let mut #cx_varname = #kael::TestAppContext::build(
                                dispatcher.clone(),
                                Some(stringify!(#outer_fn_name)),
                            );
                        ));
                        cx_teardowns.extend(quote!(
                            dispatcher.run_until_parked();
                            #cx_varname.executor().forbid_parking();
                            #cx_varname.quit();
                            dispatcher.run_until_parked();
                        ));
                        inner_fn_args.extend(quote!(&mut #cx_varname,));
                        continue;
                    }
                }
            }

            return Err(error_with_message("invalid function signature", arg));
        }

        parse_quote! {
            #[test]
            fn #outer_fn_name() {
                #inner_fn

                #kael::run_test(
                    #num_iterations,
                    &[#seeds],
                    #max_retries,
                    &mut |dispatcher, _seed| {
                        let executor = #kael::BackgroundExecutor::new(::std::sync::Arc::new(dispatcher.clone()));
                        #cx_vars
                        executor.block_test(#inner_fn_name(#inner_fn_args));
                        #cx_teardowns
                    },
                    #on_failure_fn_name
                );
            }
        }
    } else {
        // Pass to the test function the number of app contexts that it needs,
        // based on its parameter list.
        let mut cx_vars = proc_macro2::TokenStream::new();
        let mut cx_teardowns = proc_macro2::TokenStream::new();
        let mut inner_fn_args = proc_macro2::TokenStream::new();
        for (ix, arg) in inner_fn.sig.inputs.iter().enumerate() {
            if let FnArg::Typed(arg) = arg {
                if let Type::Path(ty) = &*arg.ty {
                    let last_segment = ty.path.segments.last();

                    if let Some("StdRng") = last_segment.map(|s| s.ident.to_string()).as_deref() {
                        inner_fn_args.extend(quote!(rand::SeedableRng::seed_from_u64(_seed),));
                        continue;
                    }
                } else if let Type::Reference(ty) = &*arg.ty
                    && let Type::Path(ty) = &*ty.elem
                {
                    let last_segment = ty.path.segments.last();
                    match last_segment.map(|s| s.ident.to_string()).as_deref() {
                        Some("App") => {
                            let cx_varname = format_ident!("cx_{}", ix);
                            let cx_varname_lock = format_ident!("cx_{}_lock", ix);
                            cx_vars.extend(quote!(
                                let mut #cx_varname = #kael::TestAppContext::build(
                                   dispatcher.clone(),
                                   Some(stringify!(#outer_fn_name))
                                );
                                let mut #cx_varname_lock = #cx_varname.app.borrow_mut();
                            ));
                            inner_fn_args.extend(quote!(&mut #cx_varname_lock,));
                            cx_teardowns.extend(quote!(
                                    drop(#cx_varname_lock);
                                    dispatcher.run_until_parked();
                                    #cx_varname.update(|cx| { cx.background_executor().forbid_parking(); cx.quit(); });
                                    dispatcher.run_until_parked();
                                ));
                            continue;
                        }
                        Some("TestAppContext") => {
                            let cx_varname = format_ident!("cx_{}", ix);
                            cx_vars.extend(quote!(
                                let mut #cx_varname = #kael::TestAppContext::build(
                                    dispatcher.clone(),
                                    Some(stringify!(#outer_fn_name))
                                );
                            ));
                            cx_teardowns.extend(quote!(
                                dispatcher.run_until_parked();
                                #cx_varname.executor().forbid_parking();
                                #cx_varname.quit();
                                dispatcher.run_until_parked();
                            ));
                            inner_fn_args.extend(quote!(&mut #cx_varname,));
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            return Err(error_with_message("invalid function signature", arg));
        }

        parse_quote! {
            #[test]
            fn #outer_fn_name() {
                #inner_fn

                #kael::run_test(
                    #num_iterations,
                    &[#seeds],
                    #max_retries,
                    &mut |dispatcher, _seed| {
                        #cx_vars
                        #inner_fn_name(#inner_fn_args);
                        #cx_teardowns
                    },
                    #on_failure_fn_name,
                );
            }
        }
    };
    outer_fn.attrs.extend(inner_fn_attributes);

    Ok(TokenStream::from(quote!(#outer_fn)))
}

fn parse_usize_from_expr(expr: &Expr) -> Result<usize, syn::Error> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(int), ..
    }) = expr
    else {
        return Err(syn::Error::new(expr.span(), "expected an integer"));
    };
    int.base10_parse()
        .map_err(|_| syn::Error::new(int.span(), "failed to parse integer"))
}

fn parse_u64_from_expr(expr: &Expr) -> Result<u64, syn::Error> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(int), ..
    }) = expr
    else {
        return Err(syn::Error::new(expr.span(), "expected an integer"));
    };
    int.base10_parse()
        .map_err(|_| syn::Error::new(int.span(), "failed to parse u64 integer"))
}

fn parse_u64_array(meta_list: &MetaList) -> Result<Vec<u64>, syn::Error> {
    let mut result = Vec::new();
    let tokens = &meta_list.tokens;
    let parser = |input: ParseStream| {
        let exprs = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
        for expr in exprs {
            if let Expr::Lit(ExprLit {
                lit: Lit::Int(int), ..
            }) = expr
            {
                result.push(int.base10_parse::<u64>()?);
            } else {
                return Err(syn::Error::new(expr.span(), "expected an integer"));
            }
        }
        Ok(())
    };
    syn::parse::Parser::parse2(parser, tokens.clone())?;
    Ok(result)
}

fn error_with_message(message: &str, spanned: impl Spanned) -> TokenStream {
    error_to_stream(syn::Error::new(spanned.span(), message))
}

fn error_to_stream(err: syn::Error) -> TokenStream {
    TokenStream::from(err.into_compile_error())
}

#[cfg(test)]
mod tests {
    use super::Args;

    #[test]
    fn invalid_failure_paths_return_diagnostics_instead_of_panicking() {
        for tokens in [
            quote::quote!(on_failure = ""),
            quote::quote!(on_failure = "bad-name"),
        ] {
            assert!(syn::parse2::<Args>(tokens).is_err());
        }
    }

    #[test]
    fn duplicate_scalar_arguments_are_rejected() {
        assert!(syn::parse2::<Args>(quote::quote!(iterations = 1, iterations = 2)).is_err());
        assert!(syn::parse2::<Args>(quote::quote!(seed = 1, seed = 2)).is_err());
    }

    #[test]
    fn zero_iterations_are_rejected() {
        assert!(syn::parse2::<Args>(quote::quote!(iterations = 0)).is_err());
    }

    #[test]
    fn seeds_accept_the_full_u64_range() {
        let args = syn::parse2::<Args>(quote::quote!(seed = 18446744073709551615)).unwrap();
        assert_eq!(args.seeds, vec![u64::MAX]);
    }
}
