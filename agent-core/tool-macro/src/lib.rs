use proc_macro::TokenStream;
use syn::{ItemFn, parse_macro_input};

mod config;
mod expand;
mod parameters;

/// Generate a tool and add its factory to an automatically discoverable group.
/// Both `created_at = <fixed Unix seconds>` and `version = "..."` are required.
/// Use `read_only`, `group = "debug"`, and `output = "text"` as needed.
/// Mark one shared state parameter `#[context] state: &State`; `Option<T>`
/// parameters are optional, and `#[arg(description = "...")]` documents inputs.
#[proc_macro_attribute]
pub fn tool(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    expand::tool(attribute.into(), function)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
