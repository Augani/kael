#![cfg_attr(not(target_os = "windows"), allow(unused))]
#![allow(clippy::test_attr_in_doctest)]

#[cfg(feature = "perf-enabled")]
use perf::*;
use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{ItemFn, LitStr, parse_macro_input, parse_quote};

/// A macro used in tests for cross-platform path string literals in tests. On Windows it replaces
/// `/` with `\\` and adds `C:` to the beginning of absolute paths. On other platforms, the path is
/// returned unmodified.
///
/// # Example
/// ```rust
/// use util_macros::path;
///
/// let path = path!("/Users/user/file.txt");
/// #[cfg(target_os = "windows")]
/// assert_eq!(path, "C:\\Users\\user\\file.txt");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(path, "/Users/user/file.txt");
/// ```
#[proc_macro]
pub fn path(input: TokenStream) -> TokenStream {
    let path = parse_macro_input!(input as LitStr);
    let mut path = path.value();

    #[cfg(target_os = "windows")]
    {
        path = path.replace("/", "\\");
        if path.starts_with("\\") {
            path = format!("C:{}", path);
        }
    }

    TokenStream::from(quote! {
        #path
    })
}

/// This macro replaces the path prefix `file:///` with `file:///C:/` for Windows.
/// But if the target OS is not Windows, the URI is returned as is.
///
/// # Example
/// ```rust
/// use util_macros::uri;
///
/// let uri = uri!("file:///path/to/file");
/// #[cfg(target_os = "windows")]
/// assert_eq!(uri, "file:///C:/path/to/file");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(uri, "file:///path/to/file");
/// ```
#[proc_macro]
pub fn uri(input: TokenStream) -> TokenStream {
    let uri = parse_macro_input!(input as LitStr);
    let uri = uri.value();

    #[cfg(target_os = "windows")]
    let uri = uri.replace("file:///", "file:///C:/");

    TokenStream::from(quote! {
        #uri
    })
}

/// This macro replaces the line endings `\n` with `\r\n` for Windows.
/// But if the target OS is not Windows, the line endings are returned as is.
///
/// # Example
/// ```rust
/// use util_macros::line_endings;
///
/// let text = line_endings!("Hello\nWorld");
/// #[cfg(target_os = "windows")]
/// assert_eq!(text, "Hello\r\nWorld");
/// #[cfg(not(target_os = "windows"))]
/// assert_eq!(text, "Hello\nWorld");
/// ```
#[proc_macro]
pub fn line_endings(input: TokenStream) -> TokenStream {
    let text = parse_macro_input!(input as LitStr);
    let text = text.value();

    #[cfg(target_os = "windows")]
    let text = text.replace("\n", "\r\n");

    TokenStream::from(quote! {
        #text
    })
}

#[cfg(not(feature = "perf-enabled"))]
#[derive(Default, Clone, Copy)]
enum Importance {
    Critical,
    Important,
    #[default]
    Average,
    Iffy,
    Fluff,
}

#[cfg(not(feature = "perf-enabled"))]
impl std::fmt::Display for Importance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Importance::Critical => write!(f, "Critical"),
            Importance::Important => write!(f, "Important"),
            Importance::Average => write!(f, "Average"),
            Importance::Iffy => write!(f, "Iffy"),
            Importance::Fluff => write!(f, "Fluff"),
        }
    }
}

#[derive(Default)]
struct PerfArgs {
    /// How many times to loop a test before rerunning the test binary. If left
    /// empty, the test harness will auto-determine this value.
    iterations: Option<syn::Expr>,
    /// How much this test's results should be weighed when comparing across runs.
    /// If unspecified, defaults to `WEIGHT_DEFAULT` (50).
    weight: Option<syn::Expr>,
    /// How relevant a benchmark is to overall performance. See docs on the enum
    /// for details. If unspecified, `Average` is selected.
    importance: Importance,
}

#[warn(clippy::all, clippy::pedantic)]
impl PerfArgs {
    /// Parses attribute arguments into a `PerfArgs`.
    fn parse_into(&mut self, meta: syn::meta::ParseNestedMeta) -> syn::Result<()> {
        if meta.path.is_ident("iterations") {
            self.iterations = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("weight") {
            self.weight = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("critical") {
            self.importance = Importance::Critical;
        } else if meta.path.is_ident("important") {
            self.importance = Importance::Important;
        } else if meta.path.is_ident("average") {
            // This shouldn't be specified manually, but oh well.
            self.importance = Importance::Average;
        } else if meta.path.is_ident("iffy") {
            self.importance = Importance::Iffy;
        } else if meta.path.is_ident("fluff") {
            self.importance = Importance::Fluff;
        } else {
            return Err(syn::Error::new_spanned(meta.path, "unexpected identifier"));
        }
        Ok(())
    }
}

/// Marks a test as perf-sensitive, to be triaged when checking the performance
/// of a build. This also automatically applies `#[test]`.
///
/// # Usage
/// Applying this attribute to a test marks it as average importance by default.
/// There are 5 levels of importance (`Critical`, `Important`, `Average`, `Iffy`,
/// `Fluff`); see the documentation on `Importance` for details. Add the importance
/// as a parameter to override the default (e.g. `#[perf(important)]`).
///
/// Each test also has a weight factor. This is irrelevant on its own, but is considered
/// when comparing results across different runs. By default, this is set to 50;
/// pass `weight = n` as a parameter to override this. Note that this value is only
/// relevant within its importance category.
///
/// By default, the number of iterations when profiling this test is auto-determined.
/// If this needs to be overwritten, pass the desired iteration count as a parameter
/// (`#[perf(iterations = n)]`). Note that the actual profiler may still run the test
/// an arbitrary number times; this flag just sets the number of executions before the
/// process is restarted and global state is reset.
///
/// This attribute should probably not be applied to tests that do any significant
/// disk IO, as locks on files may not be released in time when repeating a test many
/// times. This might lead to spurious failures.
///
/// # Examples
/// ```rust
/// use util_macros::perf;
///
/// #[perf]
/// fn generic_test() {
///     // Test goes here.
/// }
///
/// #[perf(fluff, weight = 30)]
/// fn cold_path_test() {
///     // Test goes here.
/// }
/// ```
///
/// This also works with `#[kael::test]`s, though in most cases it shouldn't
/// be used with automatic iterations.
/// ```rust,ignore
/// use util_macros::perf;
///
/// #[perf(iterations = 1, critical)]
/// #[kael::test]
/// fn oneshot_test(_cx: &mut kael::TestAppContext) {
///     // Test goes here.
/// }
/// ```
#[proc_macro_attribute]
#[warn(clippy::all, clippy::pedantic)]
pub fn perf(our_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut args = PerfArgs::default();
    let parser = syn::meta::parser(|meta| PerfArgs::parse_into(&mut args, meta));
    parse_macro_input!(our_attr with parser);

    let ItemFn {
        attrs: mut attrs_main,
        vis,
        sig: sig_main,
        block,
    } = parse_macro_input!(input as ItemFn);
    if !attrs_main
        .iter()
        .any(|a| Some(&parse_quote!(test)) == a.path().segments.last())
    {
        attrs_main.push(parse_quote!(#[test]));
    }
    attrs_main.push(parse_quote!(#[allow(non_snake_case)]));

    #[cfg(feature = "perf-enabled")]
    let fns = {
        #[allow(clippy::wildcard_imports, reason = "We control the other side")]
        use consts::*;

        let mut sig_main = sig_main;
        let mut new_ident_main = sig_main.ident.to_string();
        let mut new_ident_meta = new_ident_main.clone();
        new_ident_main.push_str(SUF_NORMAL);
        new_ident_meta.push_str(SUF_MDATA);

        let new_ident_main = syn::Ident::new(&new_ident_main, sig_main.ident.span());
        sig_main.ident = new_ident_main;

        let new_ident_meta = syn::Ident::new(&new_ident_meta, sig_main.ident.span());
        let sig_meta = parse_quote!(fn #new_ident_meta());
        let attrs_meta = parse_quote!(#[test] #[allow(non_snake_case)]);

        let block_main = {
            parse_quote!({
                let iter_count = std::env::var(#ITER_ENV_VAR).unwrap().parse::<usize>().unwrap();
                for _ in 0..iter_count {
                    #block
                }
            })
        };
        let importance = format!("{}", args.importance);
        let block_meta = {
            let q_iter = if let Some(iter) = args.iterations {
                quote! {
                    println!("{} {} {}", #MDATA_LINE_PREF, #ITER_COUNT_LINE_NAME, #iter);
                }
            } else {
                quote! {}
            };
            let weight = args
                .weight
                .unwrap_or_else(|| parse_quote! { #WEIGHT_DEFAULT });
            parse_quote!({
                #q_iter
                println!("{} {} {}", #MDATA_LINE_PREF, #WEIGHT_LINE_NAME, #weight);
                println!("{} {} {}", #MDATA_LINE_PREF, #IMPORTANCE_LINE_NAME, #importance);
                println!("{} {} {}", #MDATA_LINE_PREF, #VERSION_LINE_NAME, #MDATA_VER);
            })
        };

        vec![
            ItemFn {
                attrs: attrs_main,
                vis: vis.clone(),
                sig: sig_main,
                block: block_main,
            },
            ItemFn {
                attrs: attrs_meta,
                vis,
                sig: sig_meta,
                block: block_meta,
            },
        ]
    };

    #[cfg(not(feature = "perf-enabled"))]
    let fns = vec![ItemFn {
        attrs: attrs_main,
        vis,
        sig: sig_main,
        block,
    }];

    fns.into_iter()
        .flat_map(|f| TokenStream::from(f.into_token_stream()))
        .collect()
}
