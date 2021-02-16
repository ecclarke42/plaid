use std::sync::Arc;

use crate::{HttpResponse, RequestContext};

#[async_trait]
pub trait Middleware<GlobalCtx, LocalCtx>
where
    Self: Send + Sync + 'static,
    GlobalCtx: Send + Sync + 'static,
    LocalCtx: Send + Sync + 'static,
{
    async fn call(&self, context: &mut RequestContext<GlobalCtx, LocalCtx>) -> HttpResponse;
}

pub trait ToMiddleware<GlobalCtx, LocalCtx> {
    fn wrap(
        self,
        next: Arc<dyn Middleware<GlobalCtx, LocalCtx>>,
    ) -> Arc<dyn Middleware<GlobalCtx, LocalCtx>>;
}

// TODO: re-doc middlewares

// TODO: gzip/deflate/brotli? general compression middleware
//https://github.com/seanmonstar/warp/blob/c983a2d0571d5de5c7f190cac6d0baa5133f9638/src/filters/compression.rs
