use quote::quote;

pub fn transform_stream(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let Input {
        client_block_attrs,
        client_struct,
        client_field,
        client_error,
        router_fn_attrs,
        router_fn,
        router_ty,
        routes,
    } = syn::parse_macro_input!(input as Input);

    let (client_defs, route_parts): (Vec<_>, Vec<_>) = routes
        .into_iter()
        .map(|r| r.prepare(client_field.clone(), client_error.clone()))
        .unzip();

    let client_block_attrs = if let Some(attrs) = client_block_attrs {
        quote! {
            #(#attrs)*
        }
    } else {
        quote! {}
    };

    let router_fn_attrs = if let Some(attrs) = router_fn_attrs {
        quote! {
            #(#attrs)*
        }
    } else {
        quote! {}
    };

    let stream = quote! {

        #client_block_attrs
        impl #client_struct {
            #(
                #client_defs
            )*
        }

        #router_fn_attrs
        pub fn #router_fn() -> #router_ty {
            let mut router = <#router_ty>::new();
            #(
                #route_parts
            )*
            router
        }

    };

    stream.into()
}

#[derive(Debug)]
struct Input {
    client_block_attrs: Option<Vec<syn::Attribute>>,
    client_struct: syn::Path,
    client_field: syn::Ident,
    client_error: syn::Path,

    router_fn_attrs: Option<Vec<syn::Attribute>>,
    router_fn: syn::Ident,
    router_ty: syn::Type,

    routes: Vec<Route>,
}

impl syn::parse::Parse for Input {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        let content;
        let route_defs;
        syn::braced!(content in input);
        syn::bracketed!(route_defs in input);

        let client_block_attrs = if content.peek(syn::Token![#]) {
            Some(content.call(syn::Attribute::parse_outer)?)
        } else {
            None
        };
        let client_struct = content.parse()?;
        let client_fields;
        syn::braced!(client_fields in content);
        let client_field = client_fields.parse()?;
        content.parse::<syn::Token![->]>()?;
        let client_error = content.parse()?;
        content.parse::<syn::Token![;]>()?;

        let router_fn_attrs = if content.peek(syn::Token![#]) {
            Some(content.call(syn::Attribute::parse_outer)?)
        } else {
            None
        };
        let router_fn = content.parse()?;
        let _empty;
        syn::parenthesized!(_empty in content);
        content.parse::<syn::Token![->]>()?;
        let router_ty = content.parse()?;
        content.parse::<syn::Token![;]>()?;

        let mut routes = Vec::new();
        while !route_defs.is_empty() {
            routes.push(route_defs.parse()?)
        }

        Ok(Input {
            client_block_attrs,
            client_struct,
            client_field,
            client_error,
            router_fn_attrs,
            router_fn,
            router_ty,
            routes,
        })
    }
}

#[derive(Debug)]
struct Route {
    client_fn: syn::Ident,
    client_args: FieldSet,
    handler: syn::Path,
    result: Mime,
    method: syn::Ident,
    path_parts: syn::punctuated::Punctuated<PathPart, syn::Token![/]>,
}

impl syn::parse::Parse for Route {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        let client_fn = input.parse()?;

        let client_arg_list;
        syn::parenthesized!(client_arg_list in input);

        let client_args = FieldSet::from_fields(
            client_arg_list
                .parse_terminated::<Field, syn::Token![,]>(Field::parse)?
                .into_iter(),
        );

        input.parse::<syn::Token![=>]>()?;
        let handler = input.parse()?;
        input.parse::<syn::Token![=>]>()?;

        let result = if input.peek(syn::token::Paren) {
            let _empty;
            syn::parenthesized!(_empty in input);
            Mime::Empty
        } else {
            let result_ty = input.parse::<syn::PathSegment>()?;
            let outer = result_ty.ident;
            let inner = match result_ty.arguments {
                syn::PathArguments::None => None,
                syn::PathArguments::AngleBracketed(bracketed) => {
                    let mut args = bracketed.args.into_iter();
                    let arg = args.next().map(|arg| {
                        if let syn::GenericArgument::Type(ty) = arg {
                            ty
                        } else {
                            panic!("Result MIME could not be parsed with an inner type");
                        }
                    });
                    if args.next().is_some() {
                        panic!("Result MIME cannot accept multiple arguments");
                    }
                    arg
                }
                syn::PathArguments::Parenthesized(_) => {
                    panic!("Result arguments should not be parenthesized")
                }
            };
            Mime::new(outer, inner)
        };
        let inner;
        syn::braced!(inner in input);
        let method = inner.parse()?;

        let path_parts = inner.parse_terminated(PathPart::parse)?;

        Ok(Route {
            client_fn,
            client_args,
            handler,
            result,
            method,
            path_parts,
        })
    }
}

impl Route {
    fn prepare(
        &self,
        client_field: syn::Ident,
        client_err: syn::Path,
    ) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
        (
            self.prepare_client_impl(client_field, client_err),
            self.prepare_router_parts(),
        )
    }

    fn prepare_client_impl(
        &self,
        client_field: syn::Ident,
        client_err: syn::Path,
    ) -> proc_macro2::TokenStream {
        let Route {
            client_fn,
            client_args,
            handler: _,
            result,
            method,
            path_parts,
        } = self;

        let method = syn::Ident::new(&method.to_string().to_lowercase(), method.span());

        // Args order should be (path params, body, query, token)
        let mut args = Vec::new();
        let mut request_methods = Vec::new();

        let mut path_segments = Vec::new();
        for part in path_parts {
            match part {
                PathPart::Literal(lit) => {
                    path_segments.push(quote! { .push(#lit) });
                }
                PathPart::Param {
                    ident, client_ty, ..
                } => {
                    args.push(quote! { #ident: #client_ty });
                    path_segments.push(quote! { .push(&format!("{}", #ident)) });
                }
            }
        }

        let url_def = quote! {
            let mut url = self.root.clone();
            url
                .path_segments_mut()
                .map_err(|_| crate::client::Error::CannotBeBase)?
                #(#path_segments)*;
        };

        if let Some((ident, mime)) = &client_args.body {
            match mime {
                Mime::Json(inner) => {
                    request_methods.push(quote! { request = request.json(#ident); });
                    args.push(quote! {
                        #ident: &#inner
                    });
                }
                Mime::Bytes => {
                    request_methods.push(quote! { request = request.body(#ident); });
                    let ty = syn::Type::Verbatim(quote! { [u8] });
                    args.push(quote! {
                        #ident: &#ty
                    });
                }
                Mime::Empty => {}
            }
        }

        let query_def = if let Some((ident, ty)) = &client_args.query {
            args.push(quote! { #ident: #ty });
            quote! {
                let query = serde_urlencoded::to_string(#ident)?;
                url.set_query(Some(&query));
            }
        } else {
            quote! {}
        };

        if let Some((ident, ty)) = &client_args.auth {
            let ty = match ty {
                AuthType::Bearer(ty) => {
                    request_methods.push(quote! { request = request.bearer_auth(#ident); });
                    ty.clone()
                }

                AuthType::MaybeBearer(ty) => {
                    request_methods.push(quote! {
                        if let Some(token) = #ident {
                            request = request.bearer_auth(token);
                        }
                    });
                    syn::Type::Verbatim(quote! { Option<#ty> })
                }
            };
            args.push(quote! {
                #ident: #ty
            })
        }

        let (handle, result) = match result {
            Mime::Json(ty) => (quote! { handle_json_response }, quote! { #ty }),
            Mime::Bytes => (quote! { handle_bytes_response }, quote! { Vec<u8> }),
            Mime::Empty => (quote! { handle_empty_response }, quote! { () }),
        };

        quote! {
            pub async fn #client_fn(&self #(, #args)*) -> Result<#result, #client_err> {
                #url_def
                #query_def
                let mut request = self.#client_field.#method(url);
                    #( #request_methods )*
                let response = request.send().await.map_err(#client_err::Send)?;

                // TODO: module?
                Self::#handle(response).await
            }
        }
    }

    fn prepare_router_parts(&self) -> proc_macro2::TokenStream {
        let Route {
            handler,
            method,
            path_parts,
            ..
        } = self;

        let span = path_parts
            .first()
            .expect("at least one path segment required")
            .span();

        let path = path_parts
            .iter()
            .map(|part| match part {
                PathPart::Literal(lit) => lit.value(),
                PathPart::Param {
                    ident, router_ty, ..
                } => {
                    format!(":{}{{{}}}", ident.to_string(), router_ty)
                }
            })
            .collect::<Vec<_>>();

        let path = syn::LitStr::new(&format!("/{}", path.join("/")), span);

        quote! {
            router.add(vec![plaid::Method::#method], #path, #handler{});
        }
    }
}

#[derive(Debug)]
struct FieldSet {
    auth: Option<(syn::Ident, AuthType)>, // token also points to auth
    body: Option<(syn::Ident, Mime)>,
    query: Option<(syn::Ident, syn::Type)>,
}

impl Default for FieldSet {
    fn default() -> Self {
        Self {
            auth: None,
            body: None,
            query: None,
        }
    }
}

impl FieldSet {
    fn from_fields<I: Iterator<Item = Field>>(fields: I) -> Self {
        let mut set = FieldSet::default();
        for field in fields {
            let name = field.name;
            match field.ty {
                FieldType::Body(mime) => {
                    if set.body.is_some() {
                        panic!("body cannot be defined twice");
                    }
                    set.body = Some((name, mime));
                }

                FieldType::Auth(ty) => {
                    if set.auth.is_some() {
                        panic!("auth cannot be defined twice");
                    }
                    set.auth = Some((name, ty));
                }

                FieldType::Query(ty) => {
                    if set.query.is_some() {
                        panic!("query cannot be defined twice");
                    }
                    set.query = Some((name, ty));
                }
            }
        }
        set
    }
}

// #[derive(Debug, Hash, PartialEq, Eq)]
// enum Field {
//     Body,
// }

#[derive(Debug)]
struct Field {
    name: syn::Ident,
    ty: FieldType,
}

impl Field {
    fn one_inner_type(arguments: syn::PathArguments) -> Option<syn::Type> {
        match arguments {
            syn::PathArguments::None => None,
            syn::PathArguments::AngleBracketed(bracketed) => {
                let mut args = bracketed.args.into_iter();
                let arg = args.next().map(|arg| {
                    if let syn::GenericArgument::Type(ty) = arg {
                        ty
                    } else {
                        panic!("Field argument could not be parsed as an inner type");
                    }
                });
                if args.next().is_some() {
                    panic!("Fields cannot accept multiple arguments");
                }
                arg
            }
            syn::PathArguments::Parenthesized(_) => {
                panic!("Field arguments should not be parenthesized")
            }
        }
    }
}

#[derive(Debug)]
enum FieldType {
    Body(Mime),
    Auth(AuthType),
    Query(syn::Type),
}

#[derive(Debug)]
enum Mime {
    Json(syn::Type),
    Bytes,
    Empty,
}

impl Mime {
    fn new(outer: syn::Ident, inner: Option<syn::Type>) -> Self {
        match outer.to_string().as_str() {
            "Json" => Mime::Json(inner.expect("MIME Json requires an innner type")),
            "Bytes" => {
                if inner.is_some() {
                    panic!("MIME Bytes does not take an inner type");
                }
                Mime::Bytes
            }
            "Empty" => {
                if inner.is_some() {
                    panic!("MIME Empty does not take an inner type");
                }
                Mime::Empty
            }
            _ => panic!("Unknown MIME: {}", outer),
        }
    }
}

#[derive(Debug)]
enum AuthType {
    Bearer(syn::Type),
    MaybeBearer(syn::Type),
}

impl AuthType {
    fn new(outer: syn::Ident, inner: Option<syn::Type>) -> Self {
        match outer.to_string().as_str() {
            "Bearer" => {
                AuthType::Bearer(inner.unwrap_or_else(|| syn::Type::Verbatim(quote! { &str })))
            }

            "MaybeBearer" => {
                AuthType::MaybeBearer(inner.unwrap_or_else(|| syn::Type::Verbatim(quote! { &str })))
            }

            _ => panic!("Unknown auth type"),
        }
    }
}

impl syn::parse::Parse for Field {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        let name = input.parse::<syn::Ident>()?;
        let ty = match name.to_string().as_str() {
            "body" => {
                input.parse::<syn::Token![:]>()?;
                let field_type = input.parse::<syn::PathSegment>()?;
                let outer = field_type.ident;
                let inner = Field::one_inner_type(field_type.arguments);
                FieldType::Body(Mime::new(outer, inner))
            }

            "auth" | "token" => {
                // TODO: other authtypes
                input.parse::<syn::Token![:]>()?;
                let field_type = input.parse::<syn::PathSegment>()?;
                let outer = field_type.ident;
                let inner = Field::one_inner_type(field_type.arguments);
                FieldType::Auth(AuthType::new(outer, inner))
            }

            "query" => {
                input.parse::<syn::Token![:]>()?;
                let ty = input.parse::<syn::Type>()?;
                FieldType::Query(ty)
            }

            other => panic!("Unknown field type: {}", other),
        };
        Ok(Field { name, ty })
    }
}

#[derive(Debug)]
enum PathPart {
    Literal(syn::LitStr),
    Param {
        ident: syn::Ident,
        client_ty: syn::Path,
        router_ty: String,
    },
}

impl PathPart {
    fn span(&self) -> proc_macro2::Span {
        match self {
            Self::Literal(inner) => inner.span(),
            Self::Param { ident, .. } => ident.span(),
        }
    }
}

const ROUTER_TYPES: &[&str] = &["u32", "i32", "String"];

impl syn::parse::Parse for PathPart {
    fn parse(input: syn::parse::ParseStream) -> syn::parse::Result<Self> {
        if input.peek(syn::LitStr) {
            Ok(PathPart::Literal(input.parse()?))
        } else {
            let ident = input.parse()?;
            let types;
            syn::braced!(types in input);
            let paths =
                types.parse_terminated::<syn::Path, syn::Token![|]>(syn::Path::parse_mod_style)?;
            let (client_ty, router_ty) = match paths.len() {
                0 => panic!("path params must have at least one type"),
                1 => {
                    let path = paths.into_iter().next().unwrap();
                    (path.clone(), path)
                }
                2 => {
                    let mut iter = paths.into_iter();
                    (iter.next().unwrap(), iter.next().unwrap())
                }
                greater => panic!("path params accept at most two types (found: {})", greater),
            };

            let router_ty = ROUTER_TYPES
                .iter()
                .find_map(|&ty| {
                    if router_ty.is_ident(ty) {
                        Some(ty.to_string())
                    } else {
                        None
                    }
                })
                .expect("unsupported router type");

            Ok(PathPart::Param {
                ident,
                client_ty,
                router_ty,
            })
        }
    }
}
