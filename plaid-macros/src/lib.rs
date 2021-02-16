extern crate darling;

use heck::CamelCase;
use quote::quote;

mod handler;

/// Create a `plaid` handler (a struct implementing the `Handler` trait) from
/// a function.
/// TODO: usage (global_ctx, local_ctx override arg parsing)
#[proc_macro_attribute]
pub fn handler(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    // Parse attribute arguments
    let args = handler::Args::from(syn::parse_macro_input!(args as syn::AttributeArgs));

    // Parse input
    let func = handler::Signature::from(syn::parse_macro_input!(input as syn::ItemFn));

    // Combine Args and Signature to get output parameters
    let struct_ident = {
        let name = if let Some(name) = args.name {
            name
        } else {
            func.name.to_string().to_camel_case()
        };
        syn::Ident::new(&name, func.name.span())
    };

    let (global_ctx_type, local_ctx_type) = match (args.global_ctx, args.local_ctx) {
        (Some(global), Some(local)) => {
            // Reparse idents as a type
            let tokens = proc_macro::TokenStream::from(quote! { #global });
            let global = syn::parse_macro_input!(tokens as syn::Type);
            let tokens = proc_macro::TokenStream::from(quote! { #local });
            let local = syn::parse_macro_input!(tokens as syn::Type);
            (global, local)
        }
        (Some(_), None) | (None, Some(_)) => {
            panic!("If overriding `global_ctx` and `local_ctx`, both must be specified")
        }
        (None, None) => func.parse_ctx_types(),
    };

    let err_type = if let Some(ty) = args.error {
        // Reparse error ident as a type
        let tokens = proc_macro::TokenStream::from(quote! { #ty });
        syn::parse_macro_input!(tokens as syn::Type)
    } else {
        func.parse_err_type()
    };

    // If desired, generate a #[tracing::instrument(...)] statement
    let instrument_attr = if args.instrument {
        func.instrument_statement()
    } else {
        quote! {}
    };

    // Construct the new stream (and deconstructfunc fields we need)
    let handler::Signature {
        attrs,
        body,
        ctx_arg_name,
        ctx_arg_type,
        param_arg_name,
        param_arg_type,
        return_type,
        ..
    } = func;
    let output = quote! {
        pub struct #struct_ident;
        #[async_trait]
        impl Handler<#global_ctx_type, #local_ctx_type, #err_type> for #struct_ident {
            #(#attrs)*
            #instrument_attr
            async fn handle(
                &self,
                #ctx_arg_name: #ctx_arg_type,
                #param_arg_name: #param_arg_type,
            ) -> #return_type {
                #body
            }
        }
    };

    output.into()
}
