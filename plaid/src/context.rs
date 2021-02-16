use hyper::body::{Buf, Bytes};
use std::sync::Arc;

use super::HttpRequest;

/// # Request Context
///
/// The `RequestContext` struct represents the lifetime of a reqeust throughout
/// `plaid`'s handling process. A mutable reference to some `RequestContext`
/// will be passed to each middleware and then the request handler, allowing you
/// to store/share information between middlewares/the handler.
///
/// ## Global Context
///
/// The `global` field stores a reference to some global context held by the
/// server. This is read only and should include things like database/cache
/// connection pools or other references handlers might need access to.
///
/// When no global context is neccessary, just use the empty tuple type `()`.
///
/// ## Local Context
///
/// The `local` field provides access to some data specific to the handling of
/// the current request. For example, the RequestId middleware will write the
/// generated request id to this local context so that it can be used later
/// (instead of, for example, storing it in a request header and needing to
/// parse it multiple times).
///
/// Types used as local context must implement [`Default`], which is used to
/// instantiate the `local` field when a new request is recieved.
///
/// When no local context is necessary, use the empty tuple type `()` (which
/// does implement [`Default`])
///
/// ## Request
///
/// Finally, the request itself is stored in the context so it can be
/// mutated or transformed by middlewares before it is passed to the handler
/// (and to cut down on handler arguments)
///
///
/// ## Not Included
///
/// It should be noted that some information associated with the request is not
/// included in this context, like the `RequestParameters`. These are generated
/// by the router when parsing the url path, which happens after middleware
/// processing and couldn't be included here. If you need access to route
/// parameters in your middlware, please raise an issue.
///
/// TODO: Future improvement: SubRouters as well as Handlers as routes? Then
/// middleware could be stored on a particular route only
///
pub struct RequestContext<GlobalCtx, LocalCtx> {
    pub global: Arc<GlobalCtx>,
    pub local: LocalCtx,
    pub request: HttpRequest,
}

impl<G, L> RequestContext<G, L> {
    /// Consume the body of a request and return it's bytes
    pub async fn body(&mut self) -> Result<Bytes, hyper::Error> {
        hyper::body::to_bytes(self.request.body_mut()).await
    }

    /// Consume the body of a request and deserialize it to some type using
    /// `serde_json`
    pub async fn body_json<T>(&mut self) -> Result<T, JsonError>
    where
        T: serde::de::DeserializeOwned + Send,
    {
        // let body = self.body().await?;
        let reader = hyper::body::aggregate(self.request.body_mut())
            .await
            .map_err(JsonError::ReadBody)?
            .reader();
        let deserializer = &mut serde_json::Deserializer::from_reader(reader);
        let body: T =
            serde_path_to_error::deserialize(deserializer).map_err(JsonError::DeserializeBody)?;

        return Ok(body);
    }

    pub fn query<T>(&self) -> Result<T, serde_urlencoded::de::Error>
    where
        T: serde::de::DeserializeOwned + Send,
    {
        let raw_query = self.request.uri().query().unwrap_or_default();
        serde_urlencoded::from_str(raw_query)
    }
}

pub enum JsonError {
    ReadBody(hyper::Error),
    DeserializeBody(serde_path_to_error::Error<serde_json::Error>),
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonError::ReadBody(ref e) => write!(f, "Error reading body: {}", e),
            JsonError::DeserializeBody(ref e) => write!(f, "Failed to deserialize body: {}", e),
        }
    }
}
