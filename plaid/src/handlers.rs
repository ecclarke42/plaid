use std::future::Future;
use std::sync::Arc;

use crate::prelude::*;

// TODO: Fix wrapping for fn()'s

/// # Handler
///
/// The `Handler` trait defines a type that can act as a router endpoint and
/// handle requests, producing either a [`Response`] or error.
///
/// TODO: Doc. Automatic impl for closures/ fn's WIP (and just more docs for Handler trait in general)
#[async_trait]
pub trait Handler<GlobalCtx, LocalCtx, Err>: Send + Sync + 'static {
    async fn handle(
        &self,
        ctx: &mut RequestContext<GlobalCtx, LocalCtx>,
        params: RouteParameters,
    ) -> Result<Response, Err>;
}

#[async_trait]
impl<G, L, E, F, Fut> Handler<G, L, E> for F
where
    G: Send + Sync + 'static,
    L: Send + 'static,
    E: Send + 'static,
    F: Fn(&mut RequestContext<G, L>, RouteParameters) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, E>> + Send + 'static,
{
    async fn handle(
        &self,
        ctx: &mut RequestContext<G, L>,
        params: RouteParameters,
    ) -> Result<Response, E> {
        (self)(ctx, params).await
    }
}

pub type WrappedHandler<GlobalCtx, LocalCtx, Err> = Arc<dyn Handler<GlobalCtx, LocalCtx, Err>>;

/// Create a WrappedHandler from any type that implements the Handler trait
///
/// Note: For closure arguments, at least the ctx argument (`&mut RequestContext<GlobalCtx, LocalCtx>`)
/// must be type hinted for this function to identify the Handler trait.
/// For simple closures that don't need arguments, use the `closure` function
/// to wrap.
pub fn wrapped<GlobalCtx, LocalCtx, Err, F>(handler: F) -> WrappedHandler<GlobalCtx, LocalCtx, Err>
where
    F: Handler<GlobalCtx, LocalCtx, Err>,
{
    Arc::new(handler)
}

// Use same type hints as the impl Handler for F above
pub fn closure<GlobalCtx, LocalCtx, Err, F, Fut>(
    handler: F,
) -> WrappedHandler<GlobalCtx, LocalCtx, Err>
where
    GlobalCtx: Send + Sync + 'static,
    LocalCtx: Send + 'static,
    Err: Send + 'static,
    F: Fn(&mut RequestContext<GlobalCtx, LocalCtx>, RouteParameters) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Response, Err>> + Send + 'static,
{
    wrapped(handler)
}

pub fn default_error_handler<GlobalCtx, LocalCtx, Err>(
    _: &mut RequestContext<GlobalCtx, LocalCtx>,
    _: Err,
) -> Response {
    respond::status(Status::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod test {
    use crate::prelude::*;
    // use crate::{
    //     Body, Handler, Request, RequestContext, Response, RouteParameters, WrappedHandler,
    // };
    use std::sync::Arc;

    #[derive(Debug)]
    enum Error {}

    // async fn handler1(
    //     _: &mut RequestContext<(), ()>,
    //     _: RouteParameters,
    // ) -> Result<Response, Error> {
    //     Ok(respond::ok())
    // }

    // async fn handler2(
    //     _: &mut RequestContext<(), ()>,
    //     _: RouteParameters,
    // ) -> Result<Response, Error> {
    //     Ok(respond::ok())
    // }

    // TODO: add test for macro

    struct StructHandler;
    #[async_trait]
    impl Handler<(), (), Error> for StructHandler {
        async fn handle(
            &self,
            _: &mut RequestContext<(), ()>,
            _: RouteParameters,
        ) -> Result<Response, Error> {
            Ok(respond::ok())
        }
    }

    #[tokio::test]
    async fn handler_wrapping_works() {
        let hs: Vec<WrappedHandler<(), (), Error>> = vec![
            // super::wrapped(handler1), // An async function
            // super::wrapped(handler2), // A different async function
            super::wrapped(StructHandler {}),
            super::wrapped(|_: &mut RequestContext<(), ()>, _| async { Ok(respond::ok()) }), // An async closure (closures only work when &mut RequestContext is hinted)
            super::closure(|_, _| async { Ok(respond::ok()) }), // For closures without hints
            Arc::new(|_: &mut RequestContext<(), ()>, _| Box::pin(async { Ok(respond::ok()) })), // An already pinned closure
        ];
        let gctx = Arc::new(());

        for h in hs {
            let mut ctx = RequestContext {
                global: gctx.clone(),
                local: (),
                request: HttpRequest::new(hyper::Body::empty()),
            };
            let params = RouteParameters::new();
            let res = h.handle(&mut ctx, params).await;
            let res = hyper::Response::<hyper::Body>::from(res.unwrap());
            assert!(res.status().is_success())
        }
    }
}
