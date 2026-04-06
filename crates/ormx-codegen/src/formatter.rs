/// Format a token stream into a pretty-printed Rust source string.
pub fn format_token_stream(tokens: proc_macro2::TokenStream) -> String {
    let file = syn::parse2::<syn::File>(tokens).expect("generated code should be valid syntax");
    prettyplease::unparse(&file)
}
