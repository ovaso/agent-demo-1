use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Expr, FnArg, ItemFn, Lit, Meta, Pat, ReturnType, Type, parse::Parser, parse_macro_input,
};

/// 将一个自由函数转换为可注册的 rs-agent 工具。
#[proc_macro_attribute]
pub fn tool(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);

    match expand_tool(attribute, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_tool(attribute: TokenStream, function: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    if function.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "当前 #[tool] 只支持同步函数",
        ));
    }

    let config = parse_config(attribute, &function.sig.ident.to_string())?;
    let name = config.name;
    let description = config.description;
    let function_name = &function.sig.ident;
    let visibility = &function.vis;
    let arguments_type = format_ident!("__RsAgentToolArguments_{function_name}");
    let tool_type = format_ident!("__RsAgentTool_{function_name}");
    let factory_name = format_ident!("{function_name}_tool");

    let mut fields = Vec::new();
    let mut argument_names = Vec::new();
    let mut argument_types = Vec::new();

    for input in &function.sig.inputs {
        let FnArg::Typed(argument) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "#[tool] 暂不支持带 self 的方法；请标注自由函数",
            ));
        };
        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "#[tool] 的参数必须是具名标识符",
            ));
        };

        fields.push(pattern.ident.clone());
        argument_names.push(pattern.ident.clone());
        argument_types.push(argument.ty.as_ref().clone());
    }

    let parameter_definitions = fields.iter().map(|field| {
        quote! {
            ::agent_core::tool::Parameter::required(
                stringify!(#field),
                concat!("参数 ", stringify!(#field)),
            )
        }
    });

    let call = if returns_result(&function.sig.output) {
        quote! {
            let value = #function_name(#(input.#argument_names),*)
                .map_err(|error| ::agent_core::tool::ToolError::new(error.to_string()))?;
        }
    } else {
        quote! {
            let value = #function_name(#(input.#argument_names),*);
        }
    };
    let output = if config.finishes_session {
        quote! {
            ::agent_core::tool::ToolOutput::finish_session(
                ::agent_core::serde_json::from_str::<::agent_core::serde_json::Value>(&content)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or(content),
            )
        }
    } else {
        quote! { ::agent_core::tool::ToolOutput::text(content) }
    };

    Ok(quote! {
        #function

        #[derive(::agent_core::serde::Deserialize)]
        struct #arguments_type {
            #( #fields: #argument_types, )*
        }

        #visibility struct #tool_type {
            parameters: ::std::vec::Vec<::agent_core::tool::Parameter>,
        }

        impl #tool_type {
            #visibility fn new() -> Self {
                Self {
                    parameters: ::std::vec![ #( #parameter_definitions ),* ],
                }
            }
        }

        impl ::agent_core::tool::Tool for #tool_type {
            fn name(&self) -> &str {
                #name
            }

            fn description(&self) -> &str {
                #description
            }

            fn parameters(&self) -> &[::agent_core::tool::Parameter] {
                &self.parameters
            }

            fn invoke(
                &self,
                arguments: &::agent_core::tool::Arguments,
            ) -> ::std::result::Result<
                ::agent_core::tool::ToolOutput,
                ::agent_core::tool::ToolError,
            > {
                let values = arguments
                    .iter()
                    .map(|(name, value)| {
                        let value = ::agent_core::serde_json::from_str(value)
                            .unwrap_or_else(|_| ::agent_core::serde_json::Value::String(value.to_owned()));
                        (name.to_owned(), value)
                    })
                    .collect::<::agent_core::serde_json::Map<_, _>>();
                let input: #arguments_type =
                    ::agent_core::serde_json::from_value(::agent_core::serde_json::Value::Object(values))
                        .map_err(|error| ::agent_core::tool::ToolError::new(error.to_string()))?;
                #call
                let content = ::agent_core::serde_json::to_string(&value)
                    .map_err(|error| ::agent_core::tool::ToolError::new(error.to_string()))?;
                Ok(#output)
            }
        }

        #visibility fn #factory_name() -> #tool_type {
            #tool_type::new()
        }
    })
}

struct ToolConfig {
    name: String,
    description: String,
    finishes_session: bool,
}

fn parse_config(attribute: TokenStream, default: &str) -> syn::Result<ToolConfig> {
    if attribute.is_empty() {
        return Ok(ToolConfig {
            name: default.to_owned(),
            description: format!("由函数 {default} 自动生成的工具"),
            finishes_session: false,
        });
    }

    let parser = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated;
    let attributes = parser.parse(attribute)?;
    let mut config = ToolConfig {
        name: default.to_owned(),
        description: format!("由函数 {default} 自动生成的工具"),
        finishes_session: false,
    };

    for attribute in attributes {
        match attribute {
            Meta::Path(path) if path.is_ident("finish_session") => {
                config.finishes_session = true;
            }
            Meta::NameValue(name_value)
                if name_value.path.is_ident("name") || name_value.path.is_ident("description") =>
            {
                let Expr::Lit(expression) = name_value.value else {
                    return Err(syn::Error::new_spanned(
                        name_value,
                        "工具名必须是字符串字面量",
                    ));
                };
                let Lit::Str(name) = expression.lit else {
                    return Err(syn::Error::new_spanned(
                        expression,
                        "工具名必须是字符串字面量",
                    ));
                };
                if name_value.path.is_ident("name") {
                    config.name = name.value();
                } else {
                    config.description = name.value();
                }
            }
            attribute => {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "只支持 name = \"工具名\"、description = \"说明\" 与 finish_session",
                ));
            }
        }
    }

    Ok(config)
}

fn returns_result(output: &ReturnType) -> bool {
    let ReturnType::Type(_, ty) = output else {
        return false;
    };
    let Type::Path(path) = ty.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Result")
}
