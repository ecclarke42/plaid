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

mod route_definition;

/// # Plaid Route Definition
///
/// Simultaneously generate both a router and implement methods on a client
/// that connect to each endpoint.
///
/// Format:
/// ```rust
///
/// plaid::route_definition! {
///     {
///         #[attrs...]
///         ClientStruct { request_client_field } -> ClientError;
///
///         #[attrs...]
///         router_fn() -> RouterType;
///     } [
///         client_fn1(client_args) => handler1 => result {
///             METHOD "path" parts "here"
///         }
///
///         client_fn2(client_args) => handler2 => result {
///             METHOD "path" parts "here"
///         }
///
///         ...
///     ]
/// }
///
/// ```
///
/// The definition consists of two sections. First, the targets of the macro,
/// where:
///     - `ClientStruct` is the ident of the struct you want to implement
/// endpoint methods on
///     - `request_client_field` is the field on `ClientStruct` that holds an
/// http client (reqwest::Client by default)
///     - `ClientError` is the error type returned by the endpoint methods
///     - `#[attr...]` can be any attribute that needs to be passed through to
/// the client impl block or router function (for example, when the server
/// should not be built for a specific target, like wasm).
///     - `router_fn -> RouterType` is the signature of the function generated
/// to create a new router for the given endpoints
///
/// Next, a list of endpoints is defined, where:
///     - `client_fn` is the name of the client method for the given endpoint
///     - `client_args` is a list of comma seperated request properties
/// (defined below), which are also mapped to function arguments for the client
/// method
///     - `handler` is the ident of a plaid::Handler that will be instantiated
/// for the router
///     - `result` is the Ok variant of the return type of the endpoint, wrapped
/// with it's MIME type (defined further below)
///     - `METHOD` is one of `GET`, `PUT`, `PATCH`, `POST`, or `DELETE`
///     - path parts are either a string literal (for a constant route
/// component) or a parameter and type, like `param_name{type}` (much like the
/// router path input definition), where the type is a supported router
/// `plaid::routes::tree::Parameter` (`i32`, `u32`, or `String`, which will be
/// used as `&str` as an input on the client method). For extensibility, you may
/// specify two types as `param_name{client_type|router_type}` to use another
/// type of compatible serialization with a router type, which must be one of
/// the previously defined types.
///
/// TODO: extensibility of route newtypes for router to get rid of {client_type|router_type}
///
/// ## Request/Client Args
/// TODO: reference `route_definition::FieldSet`
///
/// The client method maps arguments to the http header/body through fields
/// defined in the `client_args` section as `field: type` with the following
/// options (`MIME`s are defined in the following section):
///     - `body: MIME<Type>`: Set the request body with mime type for
/// serialization. `Type` must implement `serde`'s `Serialize` and `Deserialize`
///
/// TODO: complete
///
/// ## Request/Result MIME
///
/// Requests and results must be wrapped in a MIME type, which controls
/// (de)serialization. They can be one of:
///     - `Json<T>`: "application/json", serialized with `serde_json`
///     //- `Bytes<T>`: "application/octet-stream", serialized
///     - `Bytes`: "application/octet-stream", accepted as &[u8] and returned
/// as Vec<u8>
///
/// TODO: complete
///
#[proc_macro]
pub fn route_definition(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    route_definition::transform_stream(input)
}
