mod macros;
mod tree;

use std::sync::Arc;

use crate::{handlers, prelude::*};
pub use tree::{Parameter, RouteParameters, RouteTree};

// TODO: Document options
pub struct Router<GlobalCtx, LocalCtx, Err>
where
    GlobalCtx: 'static,
    LocalCtx: 'static,
{
    pub(crate) tree: RouteTree<GlobalCtx, LocalCtx, Err>,
    pub(crate) redirect_trailing: bool,
    pub(crate) handle_options: bool, // TODO: Note this is not CORS, and will not populate CORS headers use the middleware instead
    pub(crate) error_handler:
        Arc<dyn Fn(&mut RequestContext<GlobalCtx, LocalCtx>, Err) -> Response + Send + Sync>,
}

impl<G, L, E> Default for Router<G, L, E>
where
    E: Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            tree: RouteTree::new(),
            // cors: false,
            redirect_trailing: false,
            handle_options: true,
            error_handler: Arc::new(handlers::default_error_handler),
        }
    }
}

impl<'ctx, G, L, E> Router<G, L, E>
where
    G: 'ctx,
    L: 'ctx,
    E: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self::default()
    }

    // pub fn cors(mut self, opt: bool) -> Self {
    //     self.cors = opt;
    //     self
    // }

    pub fn redirect_trailing_slash(mut self, opt: bool) -> Self {
        self.redirect_trailing = opt;
        self
    }

    pub fn handle_options(mut self, opt: bool) -> Self {
        self.handle_options = opt;
        self
    }

    pub fn on_error(
        mut self,
        handler: impl Fn(&mut RequestContext<G, L>, E) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.error_handler = Arc::new(handler);
        self
    }

    pub fn add<F: Handler<G, L, E>>(
        &mut self,
        methods: Vec<Method>,
        path: &'static str,
        handler: F,
    ) {
        self.tree
            .add_route(&methods, path, crate::handlers::wrapped(handler))
    }

    pub fn add_wrapped(
        &mut self,
        methods: Vec<Method>,
        path: &'static str,
        handler: WrappedHandler<G, L, E>,
    ) {
        self.tree.add_route(&methods, path, handler);
    }

    pub(crate) fn route(&self, path: &str, method: &Method) -> RouterResult<G, L, E> {
        let path = if self.redirect_trailing {
            path.trim_matches('/')
        } else {
            path.trim_start_matches('/')
        };

        // Find the route
        if let Some((mmap, params)) = self.tree.route_to(path) {
            // If handling options, return the option list
            if self.handle_options && method == Method::OPTIONS {
                let mut methods: Vec<Method> = mmap.keys().cloned().collect();
                methods.push(Method::OPTIONS);
                RouterResult::Options(methods)
            } else if let Some(handler) = mmap.get(method) {
                RouterResult::Found(handler.clone(), params)
            } else {
                RouterResult::MethodNotFound
            }
        } else {
            RouterResult::PathNotFound
        }
    }
}

#[async_trait]
impl<G, L, E> Middleware<G, L> for Router<G, L, E>
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    async fn call(&self, ctx: &mut RequestContext<G, L>) -> HttpResponse {
        // Process the route
        let (ctx, result) = {
            let mut ctx = ctx;
            let result = match self.route(ctx.request.uri().path(), ctx.request.method()) {
                RouterResult::Found(handler, params) => handler.handle(&mut ctx, params).await,
                RouterResult::Options(opts) => Ok(respond::options(&opts)),
                RouterResult::MethodNotFound => Ok(respond::method_not_allowed()),
                RouterResult::PathNotFound => Ok(respond::not_found()),
            };
            (ctx, result)
        };

        // Borrow checker gets angry that ctx is borrowed twice if we try to
        // call the error handler on it while "handler" is still in scope,
        // so defer the error handling until here.
        let (_ctx, response) = {
            let mut ctx = ctx;
            let resp = match result {
                Ok(response) => response,
                Err(e) => (self.error_handler)(&mut ctx, e),
            };
            (ctx, resp)
        };

        // Convert Response to Hyper
        HttpResponse::from(response)
    }
}

pub enum RouterResult<G, L, E>
where
    G: 'static,
    L: 'static,
{
    Found(WrappedHandler<G, L, E>, RouteParameters),
    PathNotFound,
    MethodNotFound,
    Options(Vec<Method>),
}

// TODO: better route search context? Some kind of struct instead of hashmap?
// TODO: RouterOptions (redirect trailing slash, cors?)

#[cfg(test)]
mod tests {

    use super::*;

    #[derive(Debug)]
    enum BasicError {}

    async fn index(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(200).unwrap()))
    }
    struct IndexHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for IndexHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            index(ctx, params).await
        }
    }

    async fn abc(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(201).unwrap()))
    }
    struct AbcHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for AbcHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            abc(ctx, params).await
        }
    }

    async fn abc_def(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(202).unwrap()))
    }
    struct AbcDefHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for AbcDefHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            abc_def(ctx, params).await
        }
    }

    async fn abc_ghi(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(203).unwrap()))
    }
    struct AbcGhiHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for AbcGhiHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            abc_ghi(ctx, params).await
        }
    }

    async fn abc_id(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(204).unwrap()))
    }
    struct AbcIdHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for AbcIdHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            abc_id(ctx, params).await
        }
    }

    async fn abc_str(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, BasicError> {
        Ok(respond::status(Status::from_u16(205).unwrap()))
    }
    struct AbcStrHandler;
    #[async_trait]
    impl Handler<(), (), BasicError> for AbcStrHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, BasicError> {
            abc_str(ctx, params).await
        }
    }

    async fn test_route(router: &Router<(), (), BasicError>, path: &str, expect_status: u16) {
        let mut ctx = RequestContext {
            global: Arc::new(()),
            local: (),
            request: HttpRequest::new(HttpBody::empty()),
        };
        if let RouterResult::Found(h, p) = router.route(path, &Method::GET) {
            let ctx = &mut ctx;
            match h.handle(ctx, p).await {
                Ok(resp) => assert_eq!(resp.status(), Status::from_u16(expect_status).unwrap()),
                Err(e) => panic!("{:?}", e),
            }
        } else {
            panic!("failed to route to /")
        }
    }

    #[tokio::test]
    async fn router_works() {
        let mut router = Router::new();
        // TODO: back to fn style
        router.add(vec![Method::GET], "/", IndexHandler {});
        router.add(vec![Method::GET], "/abc", AbcHandler {});
        router.add(vec![Method::GET], "/abc/def", AbcDefHandler {});
        router.add(vec![Method::GET], "/abc/ghi", AbcGhiHandler {});
        router.add(vec![Method::GET], "/abc/:id{i32}", AbcIdHandler {});
        router.add(vec![Method::GET], "/abc/:somestring", AbcStrHandler {});

        // println!("Router: {:#?}", router.tree);

        test_route(&router, "/", 200).await;
        test_route(&router, "/abc", 201).await;
        test_route(&router, "/abc/def", 202).await;
        test_route(&router, "/abc/ghi", 203).await;
        test_route(&router, "/abc/123", 204).await;
        test_route(&router, "/abc/striiiiing", 205).await;
    }

    #[tokio::test]
    async fn router_handles_options() {
        let mut router = Router::new().handle_options(true);
        // TODO: back to fn style
        router.add(vec![Method::GET], "/abc", IndexHandler {});
        router.add(vec![Method::PUT], "/abc", AbcHandler {});

        if let RouterResult::Options(opts) = router.route("/abc", &Method::OPTIONS) {
            assert_eq!(3, opts.len());
            for method in &[Method::GET, Method::PUT, Method::OPTIONS] {
                assert!(opts.contains(&method))
            }
        } else {
            panic!("Didn't get options result")
        }
    }
}
