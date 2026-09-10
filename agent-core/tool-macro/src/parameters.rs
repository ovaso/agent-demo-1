use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, GenericArgument, ItemFn, Meta, Pat, PathArguments, Type};

use crate::config::string_value;

pub(super) struct Parameters {
    pub context: Option<Type>,
    pub definitions: Vec<TokenStream>,
    pub decode: Vec<TokenStream>,
    pub call: Vec<TokenStream>,
}

impl Parameters {
    pub fn parse(function: &mut ItemFn) -> syn::Result<Self> {
        let mut result = Self {
            context: None,
            definitions: vec![],
            decode: vec![],
            call: vec![],
        };
        for (index, input) in function.sig.inputs.iter_mut().enumerate() {
            let FnArg::Typed(argument) = input else {
                return Err(syn::Error::new_spanned(
                    input,
                    "#[tool] 不支持 self；请标注自由函数",
                ));
            };
            let Pat::Ident(pattern) = argument.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &argument.pat,
                    "工具参数必须是具名标识符",
                ));
            };
            if pattern.by_ref.is_some() || pattern.subpat.is_some() {
                return Err(syn::Error::new_spanned(
                    pattern,
                    "工具参数不支持 ref 或子模式",
                ));
            }
            let name = pattern
                .ident
                .to_string()
                .trim_start_matches("r#")
                .to_owned();
            let mut context = false;
            let mut description = None;
            let mut remaining = Vec::new();
            for attr in std::mem::take(&mut argument.attrs) {
                if attr.path().is_ident("context") {
                    if context || !matches!(attr.meta, Meta::Path(_)) {
                        return Err(syn::Error::new_spanned(attr, "使用一个无参数的 #[context]"));
                    }
                    context = true;
                } else if attr.path().is_ident("arg") {
                    attr.parse_nested_meta(|meta| {
                        if !meta.path.is_ident("description") || description.is_some() {
                            return Err(meta.error("只支持一个 description"));
                        }
                        description = Some(string_value(&meta.value()?.parse()?)?);
                        Ok(())
                    })?;
                } else {
                    remaining.push(attr);
                }
            }
            argument.attrs = remaining;
            let ty = argument.ty.as_ref();
            if context {
                if result.context.is_some() || description.is_some() {
                    return Err(syn::Error::new_spanned(
                        argument,
                        "仅允许一个 context 参数，且不能声明 arg",
                    ));
                }
                let Type::Reference(reference) = ty else {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "context 参数必须是共享引用 &State",
                    ));
                };
                if reference.mutability.is_some() || reference.lifetime.is_some() {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "context 使用 &State，不支持可变引用或显式生命周期",
                    ));
                }
                result.context = Some(*reference.elem.clone());
                result.call.push(quote!(self.context.as_ref()));
            } else {
                let optional = type_named(ty, "Option");
                let value_type = if optional {
                    option_value(ty).unwrap_or(ty)
                } else {
                    ty
                };
                let borrowed = if let Type::Reference(reference) = value_type {
                    if reference.mutability.is_some()
                        || reference.lifetime.is_some()
                        || !type_named(&reference.elem, "str")
                    {
                        return Err(syn::Error::new_spanned(
                            value_type,
                            "借用输入只支持 &str 或 Option<&str>；注入状态使用 #[context]",
                        ));
                    }
                    true
                } else {
                    false
                };
                let constructor = if optional {
                    quote!(optional)
                } else {
                    quote!(required)
                };
                let description = description.unwrap_or_else(|| format!("参数 {name}"));
                result.definitions.push(quote! {
                    ::agent_core::tool::Parameter::#constructor(#name, #description)
                });
                let local = format_ident!("__rs_argument_{index}");
                let value = match (borrowed, optional) {
                    (true, true) => quote!(arguments.get(#name)),
                    (true, false) => {
                        quote!(::agent_core::tool::invocation::text_argument(arguments, #name)?)
                    }
                    (false, _) => {
                        quote!(::agent_core::tool::invocation::argument(arguments, #name, #optional)?)
                    }
                };
                result.decode.push(quote! { let #local: #ty = #value; });
                result.call.push(quote!(#local));
            }
        }
        Ok(result)
    }
}

fn option_value(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let PathArguments::AngleBracketed(arguments) = &path.path.segments.last()?.arguments else {
        return None;
    };
    match arguments.args.first()? {
        GenericArgument::Type(value) => Some(value),
        _ => None,
    }
}

pub(super) fn type_named(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == name))
}
