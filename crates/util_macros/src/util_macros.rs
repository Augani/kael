#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![allow(
    clippy::test_attr_in_doctest,
    reason = "the perf macro intentionally emits test functions in its doctest"
)]

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
/// use kael_util_macros::path;
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
    let native = path.value();
    let windows = windows_path(&native);
    target_string_literal(&native, &windows, path.span())
}

/// This macro replaces the path prefix `file:///` with `file:///C:/` for Windows.
/// But if the target OS is not Windows, the URI is returned as is.
///
/// # Example
/// ```rust
/// use kael_util_macros::uri;
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
    let native = uri.value();
    let windows = windows_uri(&native);
    target_string_literal(&native, &windows, uri.span())
}

/// This macro replaces the line endings `\n` with `\r\n` for Windows.
/// But if the target OS is not Windows, the line endings are returned as is.
///
/// # Example
/// ```rust
/// use kael_util_macros::line_endings;
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
    let native = text.value();
    let windows = windows_line_endings(&native);
    target_string_literal(&native, &windows, text.span())
}

fn target_string_literal(native: &str, windows: &str, span: proc_macro2::Span) -> TokenStream {
    let native = LitStr::new(native, span);
    let windows = LitStr::new(windows, span);
    quote! {
        if cfg!(target_os = "windows") { #windows } else { #native }
    }
    .into()
}

fn windows_path(path: &str) -> String {
    let path = path.replace('/', "\\");
    if path.starts_with("\\\\") {
        path
    } else if path.starts_with('\\') {
        format!("C:{path}")
    } else {
        path
    }
}

fn windows_uri(uri: &str) -> String {
    if uri.starts_with("file:////") {
        return uri.to_owned();
    }
    let Some(path) = uri.strip_prefix("file:///") else {
        return uri.to_owned();
    };
    let bytes = path.as_bytes();
    let already_has_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if already_has_drive {
        uri.to_owned()
    } else {
        format!("file:///C:/{path}")
    }
}

fn windows_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
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
    importance: Option<Importance>,
}

#[warn(clippy::all, clippy::pedantic)]
impl PerfArgs {
    /// Parses attribute arguments into a `PerfArgs`.
    fn parse_into(&mut self, meta: syn::meta::ParseNestedMeta) -> syn::Result<()> {
        if meta.path.is_ident("iterations") {
            if self.iterations.is_some() {
                return Err(meta.error("duplicate `iterations` argument"));
            }
            self.iterations = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("weight") {
            if self.weight.is_some() {
                return Err(meta.error("duplicate `weight` argument"));
            }
            self.weight = Some(meta.value()?.parse()?);
        } else if meta.path.is_ident("critical") {
            self.set_importance(Importance::Critical, &meta)?;
        } else if meta.path.is_ident("important") {
            self.set_importance(Importance::Important, &meta)?;
        } else if meta.path.is_ident("average") {
            self.set_importance(Importance::Average, &meta)?;
        } else if meta.path.is_ident("iffy") {
            self.set_importance(Importance::Iffy, &meta)?;
        } else if meta.path.is_ident("fluff") {
            self.set_importance(Importance::Fluff, &meta)?;
        } else {
            return Err(syn::Error::new_spanned(meta.path, "unexpected identifier"));
        }
        Ok(())
    }

    /// Records exactly one importance level.
    fn set_importance(
        &mut self,
        importance: Importance,
        meta: &syn::meta::ParseNestedMeta<'_>,
    ) -> syn::Result<()> {
        if self.importance.is_some() {
            return Err(meta.error("only one importance level may be specified"));
        }
        self.importance = Some(importance);
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
/// use kael_util_macros::perf;
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
/// use kael_util_macros::perf;
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
    if !attrs_main.iter().any(|attribute| {
        attribute
            .path()
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "test")
    }) {
        attrs_main.push(parse_quote!(#[test]));
    }
    attrs_main.push(parse_quote!(
        #[allow(
            non_snake_case,
            reason = "performance protocol suffixes are machine-readable"
        )]
    ));

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
        let attrs_meta = parse_quote!(
            #[test]
            #[allow(
                non_snake_case,
                reason = "performance protocol suffixes are machine-readable"
            )]
        );

        let block_main = {
            parse_quote!({
                let iter_count = ::std::env::var(#ITER_ENV_VAR)
                    .ok()
                    .and_then(|value| value.parse::<::std::num::NonZero<usize>>().ok())
                    .map(::std::num::NonZero::get)
                    .unwrap_or(1);
                for _ in 0..iter_count {
                    #block
                }
            })
        };
        let importance = format!("{}", args.importance.unwrap_or_default());
        let block_meta = {
            let q_iter = if let Some(iter) = args.iterations {
                quote! {
                    ::std::println!("{} {} {}", #MDATA_LINE_PREF, #ITER_COUNT_LINE_NAME, #iter);
                }
            } else {
                quote! {}
            };
            let weight = args
                .weight
                .unwrap_or_else(|| parse_quote! { #WEIGHT_DEFAULT });
            parse_quote!({
                #q_iter
                ::std::println!("{} {} {}", #MDATA_LINE_PREF, #WEIGHT_LINE_NAME, #weight);
                ::std::println!("{} {} {}", #MDATA_LINE_PREF, #IMPORTANCE_LINE_NAME, #importance);
                ::std::println!("{} {} {}", #MDATA_LINE_PREF, #VERSION_LINE_NAME, #MDATA_VER);
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

#[cfg(test)]
mod tests {
    use super::{PerfArgs, windows_line_endings, windows_path, windows_uri};
    use syn::parse::Parser as _;

    fn parse_perf_args(tokens: proc_macro2::TokenStream) -> syn::Result<PerfArgs> {
        let mut args = PerfArgs::default();
        let parser = syn::meta::parser(|meta| PerfArgs::parse_into(&mut args, meta));
        parser.parse2(tokens)?;
        Ok(args)
    }

    #[test]
    fn windows_path_conversion_is_independent_of_the_macro_host() {
        assert_eq!(
            windows_path("/Users/user/file.txt"),
            "C:\\Users\\user\\file.txt"
        );
        assert_eq!(windows_path("relative/file.txt"), "relative\\file.txt");
        assert_eq!(windows_path("//server/share"), "\\\\server\\share");
    }

    #[test]
    fn windows_uri_conversion_only_adds_a_missing_drive_prefix() {
        assert_eq!(
            windows_uri("file:///path/to/file"),
            "file:///C:/path/to/file"
        );
        assert_eq!(
            windows_uri("file:///D:/path/to/file"),
            "file:///D:/path/to/file"
        );
        assert_eq!(windows_uri("https://example.com"), "https://example.com");
        assert_eq!(
            windows_uri("file:////server/share"),
            "file:////server/share"
        );
    }

    #[test]
    fn windows_line_endings_do_not_double_existing_carriage_returns() {
        assert_eq!(
            windows_line_endings("one\ntwo\r\nthree"),
            "one\r\ntwo\r\nthree"
        );
    }

    #[test]
    fn perf_arguments_reject_ambiguous_duplicates() {
        for tokens in [
            quote::quote!(iterations = 1, iterations = 2),
            quote::quote!(weight = 10, weight = 20),
            quote::quote!(critical, fluff),
        ] {
            assert!(parse_perf_args(tokens).is_err());
        }
    }

    #[test]
    fn perf_arguments_accept_one_value_of_each_kind() {
        let args = parse_perf_args(quote::quote!(iterations = 4, weight = 80, important)).unwrap();

        assert!(args.iterations.is_some());
        assert!(args.weight.is_some());
        assert!(args.importance.is_some());
    }
}
