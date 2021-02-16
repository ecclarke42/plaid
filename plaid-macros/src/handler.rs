use darling::FromMeta;
use heck::SnakeCase;
use quote::quote;

#[derive(Debug, Default, darling::FromMeta)]
pub struct Args {
    #[darling(default)]
    pub instrument: bool,
    #[darling(default)]
    pub name: Option<String>,
    #[darling(default)]
    pub global_ctx: Option<syn::Ident>,
    #[darling(default)]
    pub local_ctx: Option<syn::Ident>,
    #[darling(default)]
    pub error: Option<syn::Ident>,
}

impl From<syn::AttributeArgs> for Args {
    fn from(args: syn::AttributeArgs) -> Self {
        match Args::from_list(&args) {
            Ok(args) => args,
            Err(e) => panic!("Failed to read attribute args: {}", e),
        }
    }
}

#[derive(Debug)]
pub struct Signature {
    pub name: syn::Ident,
    pub attrs: Vec<syn::Attribute>,
    pub body: syn::Block,

    pub ctx_arg_name: syn::Pat,
    pub ctx_arg_type: syn::Type,

    pub param_arg_name: syn::Pat,
    pub param_arg_type: syn::Type,

    pub return_type: syn::Type,
}

impl From<syn::ItemFn> for Signature {
    fn from(func: syn::ItemFn) -> Self {
        // Make sure signature matches:
        //  #[attrs]
        //  async fn $ident($arg1: &mut RequestContext<$global_ctx_type, $local_ctx_type>, $arg2: RouteParameters)
        //      -> Result<Response, $err_type>
        if func.sig.constness.is_some() {
            panic!("Shouldn't be a const function");
        }
        if func.sig.asyncness.is_none() {
            panic!("Should be an async function");
        }
        if func.sig.abi.is_some() {
            panic!("Can't be an abi function");
        }
        if !func.sig.generics.params.is_empty() {
            panic!("Funciton can't be generic");
        }

        // Parse Fn args
        let mut fargs = func.sig.inputs.iter();
        let (ctx_arg_name, ctx_arg_type) =
            if let Some(syn::FnArg::Typed(syn::PatType { pat, ty, .. })) = fargs.next() {
                (*pat.clone(), *ty.clone())
            } else {
                panic!("Must have at least one arg")
            };
        let (param_arg_name, param_arg_type) =
            if let Some(syn::FnArg::Typed(syn::PatType { pat, ty, .. })) = fargs.next() {
                (*pat.clone(), *ty.clone())
            } else {
                panic!("Must have at least two args")
            };

        // Parse return type
        let return_type = if let syn::ReturnType::Type(_, ty) = func.sig.output {
            (*ty).clone()
        } else {
            panic!("Return type cannot be default ()")
        };

        Signature {
            name: func.sig.ident,
            attrs: func.attrs,
            body: *func.block,

            ctx_arg_name,
            ctx_arg_type,
            param_arg_name,
            param_arg_type,
            return_type,
        }
    }
}

impl Signature {
    pub fn parse_ctx_types(&self) -> (syn::Type, syn::Type) {
        if let syn::Type::Reference(syn::TypeReference {
            mutability: Some(_),
            elem,
            ..
        }) = self.ctx_arg_type.clone()
        {
            match (*elem).clone() {
                syn::Type::Path(syn::TypePath { qself: None, path }) => {
                    if let Some(last) = path.segments.last() {
                        match last.arguments.clone() {
                            syn::PathArguments::AngleBracketed(
                                syn::AngleBracketedGenericArguments { args, .. },
                            ) => {
                                let mut arg_iter = args.iter();
                                let global = if let Some(syn::GenericArgument::Type(t)) =
                                    arg_iter.next()
                                {
                                    t.clone()
                                } else {
                                    panic!("First arg, first generic was not a `GenericArgument::Type`");
                                };

                                let local = if let Some(syn::GenericArgument::Type(t)) =
                                    arg_iter.next()
                                {
                                    t.clone()
                                } else {
                                    panic!("First arg, second generic was not a `GenericArgument::Type`");
                                };

                                (global, local)
                            }

                            _ => panic!("First arg needs type arguments"),
                        }
                    } else {
                        panic!("First arg is a path, but has no segments");
                    }
                }
                _ => panic!("Unable to parse ctx generics from first arg: {:?}", elem),
            }
        } else {
            panic!("First arg is not &mut")
        }
    }

    pub fn parse_err_type(&self) -> syn::Type {
        match self.return_type.clone() {
            syn::Type::Path(syn::TypePath { qself: None, path }) => {
                if let Some(last) = path.segments.last() {
                    match last.arguments.clone() {
                        syn::PathArguments::AngleBracketed(
                            syn::AngleBracketedGenericArguments { args, .. },
                        ) => {
                            // We only care about the second argument
                            let mut arg_iter = args.iter();
                            arg_iter.next();
                            if let Some(syn::GenericArgument::Type(t)) = arg_iter.next() {
                                t.clone()
                            } else {
                                panic!(
                                    "Result type second generic was not a `GenericArgument::Type`"
                                );
                            }
                        }
                        _ => panic!("Result type needs type arguments"),
                    }
                } else {
                    panic!("Result type is a path, but has no segments");
                }
            }
            _ => panic!(
                "Unable to parse error generic from result: {:?}",
                self.return_type
            ),
        }
    }

    pub fn instrument_statement(&self) -> proc_macro2::TokenStream {
        let name = self.name.to_string().to_snake_case() + "_route_handler";
        let ctx_arg = self.ctx_arg_name.clone();
        let param_arg = self.param_arg_name.clone();
        let args = match (ctx_arg.clone(), param_arg.clone()) {
            (syn::Pat::Ident(_), syn::Pat::Ident(_)) => quote! {
                skip(self, #ctx_arg, #param_arg), fields(?#param_arg)
            },
            (syn::Pat::Ident(_), syn::Pat::Wild(_)) => quote! {
                skip(self, #ctx_arg)
            },
            (syn::Pat::Wild(_), syn::Pat::Ident(_)) => quote! {
                skip(self, #param_arg), fields(?#param_arg)
            },
            (syn::Pat::Wild(_), syn::Pat::Wild(_)) => quote! {
                skip(self)
            },
            _ => panic!("Args must be either idents or `_`"),
        };
        quote! { #[tracing::instrument(name = #name, #args)] }
    }
}
