mod service;

use std::sync::Arc;

use crate::middleware::{Middleware, ToMiddleware};
use crate::routes::Router;

/// # Plaid Server
///
/// TODO: more description
///
/// ## Context
///
/// TODO
///
/// ## Middleware
///
/// A middlware stack is constructed by calling `.with()` on a server instance
/// with any item that implements the `Middleware` trait. Each has a `before`
/// and `after` method that can be will wrap the existing middlewares and be
/// called on the request and response respectively.
///
/// ### Middleware Call Order
///
/// Middlewares "wrap" the router, so the last middleware added will have it's
/// `before` method called on the reqeust first and it's `after` method called
/// on the response last. For example, say we introduced middlewares that altered
/// a header (say "My-Header") to a specific number:
///
/// TODO: make this a working doctest (remove no_run)
/// ```ignore
/// let server = Server::new().router().with(SetHeaderTo1).with(SetHeaderTo2)
/// ```
///
/// The request passed to the router would always have "My-Header: 1" and the
/// response returned by the server would always have "My-Header: 2".
///
/// ### Middleware Rejections
///
/// In some cases, you want to stop processing a request when a condition is met
/// in the middleware (e.g. a CORS Preflight request) and return a response.
/// The `before` method of the middleware can optionally return a response to
/// immediately issue.
///
/// NOTE: Responses returned from the `before` method of a middleware will NOT
/// be processed back through the remaining chain of middlewares. They will be
/// immediately sent.
///
/// TODO: note that router needs to be called first
pub struct Server<GlobalCtx, LocalCtx>
where
    GlobalCtx: 'static,
    LocalCtx: 'static,
{
    context: Option<Arc<GlobalCtx>>,
    // router: Option<Router<GlobalCtx, LocalCtx, Err>>,
    middleware_stack: Option<Arc<dyn Middleware<GlobalCtx, LocalCtx>>>,
}

#[derive(Debug)]
pub enum ServerError {
    Hyper(hyper::Error),

    NoContext,
    NoRouter,
}

impl<G, L> Default for Server<G, L>
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static,
{
    fn default() -> Self {
        Self {
            context: None,
            middleware_stack: None,
            // handle_error: Arc::new(crate::handlers::default_error_handler),
        }
    }
}

impl<G, L> Server<G, L>
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static + Default,
{
    pub fn new() -> Self {
        Self::default()
    }

    /// Give the server a context object to use
    pub fn context(mut self, ctx: G) -> Self {
        self.context = Some(Arc::new(ctx));
        self
    }

    /// Give the server an already wrapped context reference
    pub fn with_context(mut self, ctx: Arc<G>) -> Self {
        self.context = Some(ctx);
        self
    }

    pub fn router<E: Send + Sync + 'static>(mut self, router: Router<G, L, E>) -> Self {
        self.middleware_stack = Some(Arc::new(router));
        self
    }

    // /// To add many routes at once, the vec needs to take WrappedHanders.
    // /// For that reason, this is probably less ergonomic than defining the
    // /// router first and adding routes there using .add(...)
    // pub fn routes(
    //     mut self,
    //     routes: Vec<(Vec<Method>, &'static str, WrappedHandler<G, L, E>)>,
    // ) -> Self {
    //     if self.router.is_none() {
    //         self.router = Some(Router::new());
    //     }

    //     if let Some(ref mut router) = self.router {
    //         for (methods, path, handler) in routes {
    //             router.add_wrapped(methods, path, handler)
    //         }
    //     };
    //     self
    // }

    pub fn with<M>(mut self, middleware: M) -> Self
    where
        M: ToMiddleware<G, L> + 'static,
    {
        if let Some(stack) = self.middleware_stack {
            self.middleware_stack = Some(middleware.wrap(stack));
            self
        } else {
            panic!("Router must be set before middleware")
        }
    }

    // pub fn on_error(
    //     mut self,
    //     handler: impl Fn(&mut RequestContext<G, L>, E) -> Response + Send + Sync + 'static,
    // ) -> Self {
    //     self.handle_error = Arc::new(handler);
    //     self
    // }

    pub async fn listen<T>(self, addr: T) -> Result<(), ServerError>
    where
        T: Into<std::net::SocketAddr>,
    {
        let addr: std::net::SocketAddr = addr.into();

        if let Some(stack) = self.middleware_stack {
            let service = service::Service {
                context: self.context.ok_or(ServerError::NoContext)?,
                call_stack: stack,
            };

            let server = hyper::Server::bind(&addr).serve(service::Generator { service });
            server.await.map_err(ServerError::Hyper)
        } else {
            Err(ServerError::NoRouter)
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::prelude::*;
    use std::time::Duration;

    enum Error {}

    async fn handler(
        _: &mut RequestContext<(), ()>,
        _: RouteParameters,
    ) -> Result<Response, Error> {
        println!("received");
        Ok(respond::ok())
    }

    struct MyHandler;
    #[async_trait]
    impl Handler<(), (), Error> for MyHandler {
        async fn handle(
            &self,
            ctx: &mut RequestContext<(), ()>,
            params: RouteParameters,
        ) -> Result<Response, Error> {
            handler(ctx, params).await
        }
    }

    #[cfg(feature = "network-tests")]
    #[tokio::test] // Currently fails due to mismatched tokio versions in reqwest and this package. Should be fixed... soon?
    async fn server_works() {
        // Required because apparently tokio::test doesn't like spawning threads
        // let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        let mut router = Router::new().redirect_trailing_slash(true);
        router.add(vec![Method::GET], "/", MyHandler {}); // TODO: back to fn style

        let server = Server::new().context(()).router(router);

        let _thread_handler = tokio::spawn(async move {
            server
                .listen(([0, 0, 0, 0], 4200))
                .await
                .expect("Failed on request")
        });

        // Wait until server starts
        std::thread::sleep(Duration::from_secs(2));
        let resp = reqwest::get("http://localhost:4200")
            .await
            .expect("Failed to GET from server");

        println!("{:#?}", resp);
        assert!(resp.status().is_success());
    }

    /// This shouldn't be tested every time. Check for memory leaks, since
    /// there's some (maybe) extraneous 'static lifetimes sprinkled through the
    /// return types of our futures and I'm not 100% sure if that's going ot be
    /// an issue.
    ///
    /// After running this for ~10 mins, I didn't see any appreciable spike in
    /// memory, so we'll assume it's not an issure for now.
    #[cfg(feature = "network-tests")]
    // #[tokio::test]
    #[allow(dead_code)]
    async fn server_doesnt_leak() {
        enum Error {
            LargeErrorPayload(Vec<u8>),
        }

        async fn reply_ok(
            _: &mut RequestContext<(), ()>,
            _: RouteParameters,
        ) -> Result<Response, Error> {
            Ok(respond::ok())
        }

        struct ReplyOkHandler;
        #[async_trait]
        impl Handler<(), (), Error> for ReplyOkHandler {
            async fn handle(
                &self,
                ctx: &mut RequestContext<(), ()>,
                params: RouteParameters,
            ) -> Result<Response, Error> {
                reply_ok(ctx, params).await
            }
        }

        async fn reply_err(
            _: &mut RequestContext<(), ()>,
            _: RouteParameters,
        ) -> Result<Response, Error> {
            Err(Error::LargeErrorPayload(vec![1; 1_000_000])) // Allocate 1 MB
        }

        struct ReplyErrHandler;
        #[async_trait]
        impl Handler<(), (), Error> for ReplyErrHandler {
            async fn handle(
                &self,
                ctx: &mut RequestContext<(), ()>,
                params: RouteParameters,
            ) -> Result<Response, Error> {
                reply_err(ctx, params).await
            }
        }

        let mut router: Router<(), (), Error> = Router::new();

        // TODO: Back to fn style
        let reply_ok = ReplyOkHandler {};
        let reply_err = ReplyErrHandler {};
        crate::routes! {
            router => [
                "/ok": GET => reply_ok;
                "/err": GET => reply_err;
            ]
        }
        let server = Server::new().context(()).router(router);

        let _thread_handler = tokio::spawn(async move {
            server
                .listen(([0, 0, 0, 0], 4201)) // Make sure this is different from above test
                .await
                .expect("Failed on request")
        });

        // Wait until server starts
        std::thread::sleep(Duration::from_secs(2));

        let mut tf = true;
        let mut total_requests: usize = 0;
        while total_requests < 100_000_000 {
            // loop {
            let resp = if tf {
                reqwest::get("http://localhost:4201/ok")
            } else {
                reqwest::get("http://localhost:4201/err")
            }
            .await
            .expect("Failed to GET from server");

            assert_eq!(tf, resp.status().is_success());
            tf = !tf;
            total_requests += 1;
        }
    }
}
