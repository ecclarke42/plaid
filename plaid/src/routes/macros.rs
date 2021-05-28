/// Add a route tree to a give router. See the `router` macro for the syntax
/// of defining a route tree.
#[macro_export]
macro_rules! routes {
    {$router:ident => [
        $($rest:tt)*
    ]} => {
        $crate::__internal_routes!(@start $router, $($rest)*)
    }
}

// TODO: more docs

/// Create a router from the given route tree
///
/// The route tree can be defined from a list of items and blocks, as follows.
/// Blocks can be nested with no limit on depth.
///
/// TODO: doctest?
/// ```ignore
/// let my_router = router! [
///     "path": METHOD => handler;          // an item
///     "block_path": {                     // initiating a block
///         METHOD => handler;              // a block item
///
///         "sub_path1": METHOD => handler; // an item in a block
///
///         "sub_path": {                   // a sub-block
///             METHOD => handler;          // another block item
///         }
///     }
///```
///
/// `path` arguments must be string literals and will be concatenated with "/"
/// across parent blocks to use as an input to `RouteTree::add_route`'s `path`
/// argument.
///
/// The `METHOD` element in the above can either be a single method (all caps,
/// such as GET, PUT, POST, etc.), or multiple methods, seperated by a "|" pipe
/// charater (e.g. `PUT | POST`)
///
/// The `handler` element can be any expression that results in an item that
/// implements `Handler`. It can also be any concrete type that implements
/// `Handler` and can be instantiated with no arguments (i.e. TypeName{})
#[macro_export]
macro_rules! router {
    [
        $($routes:tt)*
    ] => {
        {
            let mut router = $crate::Router::new();
            $crate::__internal_routes!(@start router, $($routes)*);
            router
        }
    }
}

#[macro_export]
macro_rules! __internal_routes {

    (@start $router:ident, ) => {};
    (@start $router:ident, $($parts:tt)*) => {
        $crate::__internal_routes!(@munch $router; ""; $($parts)*)
    };

    // Munch an expression/ident handler entry
    (@munch $router:ident; $parent:expr; $($method:ident)|+ => $handler:expr;$($rest:tt)*) => {
        $router.add(vec![$($crate::Method::$method),*], $parent, $handler);
        $crate::__internal_routes!(@munch $router; $parent; $($rest)*)
    };

    (@munch $router:ident; $parent:expr; $path:literal: $($method:ident)|+ => $handler:expr;$($rest:tt)*) => {
        $router.add(vec![$($crate::Method::$method),*], concat!($parent, $path), $handler);
        $crate::__internal_routes!(@munch $router; $parent;$($rest)*)
    };

    // Munch a handler struct entry
    (@munch $router:ident; $parent:expr; $($method:ident)|+ => $handler:ty;$($rest:tt)*) => {
        $router.add(vec![$($crate::Method::$method),*], $parent, $handler{});
        $crate::__internal_routes!(@munch $router; $parent; $($rest)*)
    };

    (@munch $router:ident; $parent:expr; $path:literal: $($method:ident)|+ => $handler:ty;$($rest:tt)*) => {
        $router.add(vec![$($crate::Method::$method),*], concat!($parent, $path), $handler{});
        $crate::__internal_routes!(@munch $router; $parent; $($rest)*)
    };

    // Munch the next entry (as block)
    (@munch $router:ident; $parent:expr; $path:literal: { $($block_parts:tt)* }$($rest:tt)*) => {
        $crate::__internal_routes!(@munch $router; concat!($parent, $path); $($block_parts)*);
        $crate::__internal_routes!(@munch $router; $parent; $($rest)*)
    };

    // // When we concat! we need to receive an expr, not literal
    // (@munch_concat $routes:ident; $parent:expr; $($rest:tt)*) => {
    //     $crate::__internal_routes!(@munch $routes; $parent; $($rest)*)
    // };

    (@munch $router:ident; $parent:expr;) => {

    };
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::prelude::*;
    use crate::routes::RouterResult;

    enum BasicError {}

    #[crate::handler(name = "Echo")]
    async fn echo(
        _ctx: &mut RequestContext<(), ()>,
        params: crate::RouteParameters,
    ) -> Result<Response, BasicError> {
        println!("{:#?}", params);
        Ok(crate::respond::ok())
    }

    // struct EchoHandler;
    // #[async_trait]
    // impl Handler<(), (), BasicError> for EchoHandler {
    //     async fn handle(
    //         &self,
    //         ctx: &mut RequestContext<(), ()>,
    //         params: RouteParameters,
    //     ) -> Result<Response, BasicError> {
    //         echo(ctx, params).await
    //     }
    // }

    #[tokio::test]
    async fn router_macro_works() {
        let mut router: Router<(), (), BasicError> = crate::Router::new();

        // TODO: fix handler wrapping, go back to echo
        routes! (router => [
            "/": GET => Echo{};
            "/abc/:id{i32}": {
                GET => Echo;
                PUT | POST => Echo;
            }
        ]);

        let ctx = Arc::new(());

        let mut request = HttpRequest::new(HttpBody::empty());
        *request.uri_mut() = hyper::http::uri::Uri::from_static("http://localhost/abc/123");
        *request.method_mut() = Method::GET;

        let mut req_ctx = RequestContext {
            global: ctx.clone(),
            local: (),
            request,
        };

        let path = req_ctx.request.uri().path().trim_matches('/');
        match router.route(path, &Method::GET) {
            RouterResult::Found(h, params) => {
                assert!(h.handle(&mut req_ctx, params).await.is_ok())
            }
            RouterResult::Options(_) => panic!("got unexpected options"),
            RouterResult::MethodNotFound => panic!("method not found"),
            RouterResult::PathNotFound => panic!("path not found"),
        };
    }
}
