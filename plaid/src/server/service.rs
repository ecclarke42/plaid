use std::sync::Arc;

use crate::{HttpRequest, HttpResponse, Middleware, RequestContext};

type PinnedFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

pub(super) struct Service<GlobalCtx, LocalCtx>
where
    GlobalCtx: 'static,
    LocalCtx: 'static,
{
    pub(super) context: Arc<GlobalCtx>,
    pub(super) call_stack: Arc<dyn Middleware<GlobalCtx, LocalCtx>>,
}

impl<G, L> Clone for Service<G, L> {
    fn clone(&self) -> Self {
        Self {
            context: self.context.clone(),
            call_stack: self.call_stack.clone(),
        }
    }
}

impl<GlobalCtx, LocalCtx> hyper::service::Service<HttpRequest> for Service<GlobalCtx, LocalCtx>
where
    GlobalCtx: Send + Sync + 'static,
    LocalCtx: Send + Sync + 'static + Default,
{
    type Response = HttpResponse;
    type Error = hyper::Error;
    type Future = PinnedFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    /// Call the http service
    ///
    /// This uses some creating scoping to make the borrow checker happy about
    /// passing around `&mut ctx` in futures all over the place. Apparently the
    /// compiler itsn't quite smart enough to know that the `.await`s make this
    /// effectively a sync segment. (Unless I'm missing something, which I
    /// probably am...)
    fn call(&mut self, req: HttpRequest) -> Self::Future {
        let call_stack = self.call_stack.clone();
        let mut context = RequestContext {
            global: self.context.clone(),
            local: LocalCtx::default(),
            request: req,
        };
        Box::pin(async move { Ok(call_stack.call(&mut context).await) })
    }
}

pub(super) struct Generator<GlobalCtx, LocalCtx>
where
    GlobalCtx: 'static,
    LocalCtx: 'static,
{
    pub(crate) service: Service<GlobalCtx, LocalCtx>,
}

impl<T, G, L> hyper::service::Service<T> for Generator<G, L>
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static,
{
    type Response = Service<G, L>;
    type Error = hyper::Error;
    type Future = PinnedFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let svc = self.service.clone();
        Box::pin(async move { Ok(svc) })
    }
}
