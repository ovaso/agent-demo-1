use quote::quote;

use crate::expand;

#[test]
fn rejects_unsupported_signatures_and_invalid_options() {
    let cases = [
        (
            quote!(),
            quote!(
                async fn bad() {}
            ),
        ),
        (
            quote!(),
            quote!(
                unsafe fn bad() {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad<T>(x: T) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(&self) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad((x, y): (u8, u8)) {}
            ),
        ),
        (
            quote!(read_only, read_only),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(name = ""),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(group = " "),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(output = "unknown"),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(read_only, finish_session),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(finish_session, output = "tool"),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(description = 1),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(unknown),
            quote!(
                fn bad() {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[context] state: String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[context] state: &mut String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[context] state: &'static String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[context] a: &String, #[context] b: &String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(
                    #[context]
                    #[arg(description = "x")]
                    state: &String,
                ) {
                }
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(
                    #[context]
                    #[context]
                    state: &String,
                ) {
                }
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[arg(unknown = "x")] x: String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(#[arg(description = "x", description = "y")] x: String) {}
            ),
        ),
        (
            quote!(),
            quote!(
                fn bad(x: &mut str) {}
            ),
        ),
    ];
    for (attributes, function) in cases {
        let function = syn::parse2(function).unwrap();
        assert!(
            expand::tool(
                quote!(created_at = 1, version = "v1", #attributes),
                function
            )
            .is_err()
        );
    }
}

#[test]
fn preserves_cfg_on_generated_items_and_consumes_helper_attributes() {
    let function = syn::parse2(quote! {
        #[cfg(any())]
        fn inspect(#[context] context: &State, #[arg(description = "limit")] limit: Option<usize>) -> String {
            format!("{context:?}: {limit:?}")
        }
    }).unwrap();
    let expanded = expand::tool(
        quote!(created_at = 1, version = "v1", read_only, output = "text"),
        function,
    )
    .unwrap();
    let file: syn::File = syn::parse2(expanded.clone()).unwrap();
    // Function, generated type, two impls, factory and inventory submission.
    assert_eq!(file.items.len(), 6);
    let text = expanded.to_string();
    assert_eq!(text.matches("cfg (any ())").count(), 6);
    assert!(!text.contains("# [context]"));
    assert!(!text.contains("# [arg"));
}

#[test]
fn rejects_unsupported_borrowed_input_types() {
    for function in [
        quote!(
            fn bad(x: Option<&mut str>) {}
        ),
        quote!(
            fn bad(x: &String) {}
        ),
        quote!(
            fn bad(x: &'static str) {}
        ),
        quote!(
            fn bad(x: &[u8]) {}
        ),
    ] {
        assert!(
            expand::tool(
                quote!(created_at = 1, version = "v1"),
                syn::parse2(function).unwrap()
            )
            .is_err()
        );
    }
}

#[test]
fn requires_fixed_creation_time_and_nonempty_reference_version() {
    for attributes in [
        quote!(),
        quote!(created_at = 1),
        quote!(version = "v1"),
        quote!(created_at = 0, version = "v1"),
        quote!(created_at = -1, version = "v1"),
        quote!(created_at = "2026-09-10", version = "v1"),
        quote!(created_at = 1.5, version = "v1"),
        quote!(created_at = now(), version = "v1"),
        quote!(created_at = 18446744073709551616, version = "v1"),
        quote!(created_at = 1, version = " "),
        quote!(created_at = 1, created_at = 2, version = "v1"),
        quote!(created_at = 1, version = "v1", version = "v2"),
    ] {
        let function = syn::parse2(quote!(
            fn inspect() {}
        ))
        .unwrap();
        assert!(expand::tool(attributes, function).is_err());
    }
    let function = syn::parse2(quote!(
        fn inspect() {}
    ))
    .unwrap();
    let tokens = expand::tool(
        quote!(
            created_at = 1789028730,
            name = "existing_name",
            version = "v1.0.0-20260910"
        ),
        function,
    )
    .unwrap()
    .to_string();
    assert!(tokens.contains("1789028730"));
    assert!(tokens.contains("existing_name"));
    assert!(tokens.contains("v1.0.0-20260910"));
}
