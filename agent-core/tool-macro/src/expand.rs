use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, ReturnType};

use crate::{
    config::{Config, Output},
    parameters::{Parameters, type_named},
};

pub(super) fn tool(attribute: TokenStream, mut function: ItemFn) -> syn::Result<TokenStream> {
    let signature = &function.sig;
    if signature.asyncness.is_some()
        || signature.unsafety.is_some()
        || signature.abi.is_some()
        || signature.variadic.is_some()
        || !signature.generics.params.is_empty()
        || signature.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            signature,
            "#[tool] 仅支持无泛型的安全同步 Rust 自由函数",
        ));
    }
    let config = Config::parse(attribute, &function)?;
    let Parameters {
        context,
        definitions,
        decode,
        call,
    } = Parameters::parse(&mut function)?;
    let name = &config.name;
    let created_at = config.created_at;
    let version = &config.version;
    let description = &config.description;
    let group = &config.group;
    let read_only = config.read_only;
    let finish = config.finish;
    let function_name = &function.sig.ident;
    let visibility = &function.vis;
    let tool_type = format_ident!("__RsAgentTool_{function_name}");
    let factory_name = format_ident!("{function_name}_tool");
    let cfg: Vec<_> = function
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"))
        .collect();
    let (context_field, constructor_argument, context_init, factory_argument, automatic_factory) =
        if let Some(ty) = context {
            (
                quote!(context: ::std::sync::Arc<#ty>,),
                quote!(context: ::std::sync::Arc<#ty>),
                quote!(context,),
                quote!(context),
                quote! {
                    let context = context.and_then(|context| context.downcast_ref::<::std::sync::Arc<#ty>>())
                        .ok_or_else(|| ::agent_core::tool::RegistryError::MissingContext {
                            tool: #name.into(), expected: ::std::any::type_name::<#ty>(),
                        })?;
                    Ok(::std::boxed::Box::new(#factory_name(::std::sync::Arc::clone(context))))
                },
            )
        } else {
            (
                quote!(),
                quote!(),
                quote!(),
                quote!(),
                quote! {
                    let _ = context;
                    Ok(::std::boxed::Box::new(#factory_name()))
                },
            )
        };
    let result =
        matches!(&function.sig.output, ReturnType::Type(_, ty) if type_named(ty, "Result"));
    let invoke = if result {
        quote!(let value = #function_name(#(#call),*).map_err(|error| ::agent_core::tool::ToolError::new(error.to_string()))?;)
    } else {
        quote!(let value = #function_name(#(#call),*);)
    };
    let output = match config.output {
        Output::Json => quote!(::agent_core::tool::invocation::json_output(&value, #finish)),
        Output::Text if finish => quote!(Ok(::agent_core::tool::ToolOutput::finish_session(value))),
        Output::Text => quote!(Ok(::agent_core::tool::ToolOutput::text(value))),
        Output::Tool => quote!(Ok(value)),
    };
    Ok(quote! {
        #function

        #(#cfg)*
        #[allow(non_camel_case_types)]
        #visibility struct #tool_type {
            parameters: ::std::vec::Vec<::agent_core::tool::Parameter>,
            #context_field
        }

        #(#cfg)*
        impl #tool_type {
            #visibility fn new(#constructor_argument) -> Self {
                Self { parameters: ::std::vec![#(#definitions),*], #context_init }
            }
        }

        #(#cfg)*
        impl ::agent_core::tool::Tool for #tool_type {
            fn created_at(&self) -> u64 { #created_at }
            fn name(&self) -> &str { #name }
            fn version(&self) -> &str { #version }
            fn description(&self) -> &str { #description }
            fn parameters(&self) -> &[::agent_core::tool::Parameter] { &self.parameters }
            fn is_read_only(&self) -> bool { #read_only }
            fn invoke(&self, arguments: &::agent_core::tool::Arguments)
                -> ::std::result::Result<::agent_core::tool::ToolOutput, ::agent_core::tool::ToolError>
            {
                let _ = arguments;
                #(#decode)*
                #invoke
                #output
            }
        }

        #(#cfg)*
        #visibility fn #factory_name(#constructor_argument) -> #tool_type {
            #tool_type::new(#factory_argument)
        }

        #(#cfg)*
        ::agent_core::inventory::submit! {
            ::agent_core::tool::ToolRegistration {
                created_at: #created_at,
                name: #name,
                group: #group,
                factory: |context| { #automatic_factory },
            }
        }
    })
}
