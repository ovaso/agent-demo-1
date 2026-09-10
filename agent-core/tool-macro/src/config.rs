use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use syn::{Expr, ItemFn, Lit, Meta, parse::Parser};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Output {
    Json,
    Text,
    Tool,
}

pub(super) struct Config {
    pub created_at: u64,
    pub name: String,
    pub version: String,
    pub description: String,
    pub group: String,
    pub read_only: bool,
    pub finish: bool,
    pub output: Output,
}

impl Config {
    pub fn parse(attribute: TokenStream, function: &ItemFn) -> syn::Result<Self> {
        let name = function.sig.ident.to_string();
        let docs: Vec<_> = function
            .attrs
            .iter()
            .filter_map(|attr| {
                if !attr.path().is_ident("doc") {
                    return None;
                }
                let Meta::NameValue(meta) = &attr.meta else {
                    return None;
                };
                string_value(&meta.value).ok().map(|s| s.trim().to_owned())
            })
            .collect();
        let mut config = Self {
            created_at: 0,
            version: String::new(),
            description: if docs.is_empty() {
                format!("由函数 {name} 自动生成的工具")
            } else {
                docs.join("\n")
            },
            name,
            group: "default".into(),
            read_only: false,
            finish: false,
            output: Output::Json,
        };
        let parser = syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated;
        let mut seen = BTreeSet::new();
        for attribute in parser.parse2(attribute)? {
            let key = attribute
                .path()
                .get_ident()
                .map(ToString::to_string)
                .ok_or_else(|| syn::Error::new_spanned(&attribute, "工具选项必须是标识符"))?;
            if !seen.insert(key.clone()) {
                return Err(syn::Error::new_spanned(attribute, "工具选项不能重复"));
            }
            match &attribute {
                Meta::Path(_) if key == "read_only" => config.read_only = true,
                Meta::Path(_) if key == "finish_session" => config.finish = true,
                Meta::NameValue(meta) if key == "created_at" => {
                    let Expr::Lit(literal) = &meta.value else {
                        return Err(syn::Error::new_spanned(
                            &meta.value,
                            "created_at 必须是固定的正整数 Unix 秒时间戳",
                        ));
                    };
                    let Lit::Int(value) = &literal.lit else {
                        return Err(syn::Error::new_spanned(
                            &meta.value,
                            "created_at 必须是固定的正整数 Unix 秒时间戳",
                        ));
                    };
                    config.created_at = value.base10_parse()?;
                    if config.created_at == 0 {
                        return Err(syn::Error::new_spanned(
                            value,
                            "created_at 必须大于零；0 仅用于兼容旧工具定义",
                        ));
                    }
                }
                Meta::NameValue(meta) => {
                    let value = string_value(&meta.value)?;
                    match key.as_str() {
                        "name" if !value.trim().is_empty() => config.name = value,
                        "description" => config.description = value,
                        "group" if !value.trim().is_empty() => config.group = value,
                        "version" if !value.trim().is_empty() => config.version = value,
                        "output" => {
                            config.output = match value.as_str() {
                                "json" => Output::Json,
                                "text" => Output::Text,
                                "tool" => Output::Tool,
                                _ => {
                                    return Err(syn::Error::new_spanned(
                                        attribute,
                                        "output 必须为 json、text 或 tool",
                                    ));
                                }
                            }
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                attribute,
                                "未知选项或 name/group/version 为空",
                            ));
                        }
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "支持 created_at、name、version、description、group、output、read_only 与 finish_session",
                    ));
                }
            }
        }
        if !seen.contains("created_at") || !seen.contains("version") {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "#[tool] 必须显式提供 created_at 和 version；created_at 是首次加入源码时固定的 Unix 秒时间戳",
            ));
        }
        if config.finish && (config.read_only || config.output == Output::Tool) {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "finish_session 不能与 read_only 或 output = \"tool\" 同用",
            ));
        }
        Ok(config)
    }
}

pub(super) fn string_value(expression: &Expr) -> syn::Result<String> {
    if let Expr::Lit(literal) = expression
        && let Lit::Str(value) = &literal.lit
    {
        return Ok(value.value());
    }
    Err(syn::Error::new_spanned(
        expression,
        "选项值必须是字符串字面量",
    ))
}
