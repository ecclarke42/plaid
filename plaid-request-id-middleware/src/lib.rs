#![forbid(unsafe_code)]

#[macro_use]
extern crate async_trait;

use hyper::header::{HeaderName, HeaderValue};
use std::sync::Arc;

#[cfg(feature = "tracing")]
use tracing_futures::Instrument;

use plaid::{HttpResponse, Middleware, RequestContext, ToMiddleware};

// TODO: features for different id generators

/// TODO: docs, how to implement RequestIdContext
pub struct RequestId<GlobalCtx, LocalCtx> {
    config: RequestIdConfiguration,
    next: Arc<dyn Middleware<GlobalCtx, LocalCtx>>,
}

pub struct RequestIdConfiguration {
    // tracing: bool,
    mode: RequestIdMode,
}

pub const HEADER: &str = "request-id";
const UNKNOWN_ID: &str = "<unknown-id>";

impl Default for RequestIdConfiguration {
    fn default() -> Self {
        Self {
            // tracing: true,
            mode: RequestIdMode::UUID,
        }
    }
}

impl RequestIdConfiguration {
    /// Set id generation mode to UUID
    pub fn uuid_v4(mut self) -> Self {
        self.mode = RequestIdMode::UUID;
        self
    }

    // pub fn tracing(mut self, opt: bool) -> Self {
    //     self.tracing = opt;
    //     self
    // }
}

impl<G, L> ToMiddleware<G, L> for RequestIdConfiguration
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static + RequestIdContext,
{
    fn wrap(self, next: Arc<dyn Middleware<G, L>>) -> Arc<dyn Middleware<G, L>> {
        Arc::new(RequestId { config: self, next })
    }
}

impl<G, L> RequestId<G, L> {
    pub fn builder() -> RequestIdConfiguration {
        RequestIdConfiguration::default()
    }

    fn generate(&self) -> String {
        match self.config.mode {
            RequestIdMode::UUID => uuid::Uuid::new_v4().to_string(),
        }
    }
}

#[async_trait]
impl<GlobalCtx, LocalCtx> Middleware<GlobalCtx, LocalCtx> for RequestId<GlobalCtx, LocalCtx>
where
    GlobalCtx: Send + Sync + 'static,
    LocalCtx: Send + Sync + 'static + RequestIdContext,
{
    async fn call(&self, context: &mut RequestContext<GlobalCtx, LocalCtx>) -> HttpResponse {
        context.local.set_request_id(self.generate()); // TODO: use existing (if available)

        #[cfg(feature = "tracing")]
        let span = tracing::debug_span!(
            "request-lifetime",
            request_id = %context.local.request_id(),
            method = context.request.method().as_str(),
            path = context.request.uri().path()
        );

        let stack_future = self.next.call(context);

        // TODO: check if anything gets logged with TRACE when filtered to info
        #[cfg(feature = "tracing")]
        let stack_future = stack_future.instrument(span);

        let mut response = stack_future.await;

        let id = context.local.request_id();
        let id = if id.is_empty() { UNKNOWN_ID } else { &id };

        response.headers_mut().insert(
            HeaderName::from_static(HEADER),
            HeaderValue::from_str(id).unwrap_or_else(|_| HeaderValue::from_static(UNKNOWN_ID)),
        );

        response
    }
}

pub trait RequestIdContext {
    fn request_id(&self) -> String;
    fn set_request_id(&mut self, id: String);
}

enum RequestIdMode {
    UUID,
    // Base64(usize),
    // Hex(usize),
}

// TODO: tests
