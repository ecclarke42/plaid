#![forbid(unsafe_code)]

#[macro_use]
extern crate async_trait;

use hyper::{
    header,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use std::convert::TryInto;
use std::sync::Arc;

use plaid::middleware::{Middleware, ToMiddleware};
use plaid::{HttpBody, HttpRequest, HttpResponse, Method, RequestContext, Response, Status};

/// Plaid Middleware for Cross-Origin Resource Sharing Requests
///
/// This middleware handles CORS requests when the Origin header is present.
///
/// On preflight requests (on OPTIONS requests including an Origin), the
/// Access-Control-Request-Method and Access-Control-Request-Headers headers are
/// checked against the configured allowed methods and header respectively. A
/// preflight response is generated and returned, breaking the middleware chain.
pub struct CorsConfiguration {
    allow_credentials: bool,
    allow_methods: Allowable<Method>,
    allow_origins: Allowable<HeaderValue>,
    allow_headers: Bounded<HeaderName>,
    expose_headers: Bounded<HeaderName>,
    // max_age... // Maybe? see https://docs.rs/warp/0.2.5/src/warp/filters/cors.rs.html#245 for ref
    // next: Box<dyn Middleware<GlobalCtx, LocalCtx>>,
}

impl Default for CorsConfiguration {
    /// Default Cors is equivalent to Cors::any()
    fn default() -> Self {
        Self {
            allow_credentials: false,
            allow_methods: Allowable::none(),
            allow_origins: Allowable::none(),
            allow_headers: Bounded::None,
            expose_headers: Bounded::None,
        }
    }
}

const LIST_ALL_METHODS: &str = "OPTIONS, GET, POST, PUT, DELETE, HEAD, TRACE, CONNECT, PATCH";

impl CorsConfiguration {
    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }

    pub fn allow_method<T>(mut self, method: T) -> Self
    where
        T: TryInto<Method>,
    {
        let s = &mut self;
        if let Ok(method) = method.try_into() {
            s.allow_methods.add(method);
        } else {
            panic!("Failed to parse input as http method");
        }
        self
    }
    pub fn allow_methods<T>(mut self, methods: T) -> Self
    where
        T: IntoIterator,
        T::Item: TryInto<Method>,
    {
        for method in methods {
            self = self.allow_method(method);
        }
        self
    }
    pub fn allow_any_method(mut self) -> Self {
        self.allow_methods = Allowable::Any;
        self
    }

    pub fn allow_header<T>(mut self, header: T) -> Self
    where
        T: TryInto<HeaderName>,
    {
        if let Ok(header) = header.try_into() {
            self.allow_headers.add(header);
        } else {
            panic!("Failed to parse input as http header name");
        }
        self
    }
    pub fn allow_headers<T>(mut self, headers: T) -> Self
    where
        T: IntoIterator,
        T::Item: TryInto<HeaderName>,
    {
        for header in headers {
            self = self.allow_header(header);
        }
        self
    }

    pub fn allow_origin<T>(mut self, origin: T) -> Self
    where
        T: TryInto<HeaderValue>,
    {
        if let Ok(origin) = origin.try_into() {
            self.allow_origins.add(origin);
        } else {
            panic!("Failed to parse input as origin");
        }
        self
    }
    pub fn allow_origins<T>(mut self, origins: T) -> Self
    where
        T: IntoIterator,
        T::Item: TryInto<HeaderValue>,
    {
        for origin in origins {
            self = self.allow_origin(origin);
        }
        self
    }
    pub fn allow_any_origin(mut self) -> Self {
        self.allow_origins = Allowable::Any;
        self
    }

    pub fn expose_header<T>(mut self, header: T) -> Self
    where
        T: TryInto<HeaderName>,
    {
        if let Ok(header) = header.try_into() {
            self.expose_headers.add(header);
        } else {
            panic!("Failed to parse input as http header name");
        }
        self
    }
    pub fn expose_headers<T>(mut self, headers: T) -> Self
    where
        T: IntoIterator,
        T::Item: TryInto<HeaderName>,
    {
        for header in headers {
            self = self.expose_header(header);
        }
        self
    }
}

pub struct Cors<GlobalCtx, LocalCtx> {
    config: CorsConfiguration,
    next: Arc<dyn Middleware<GlobalCtx, LocalCtx>>,
}

enum RequestType {
    CorsPreflight(HeaderValue),
    CorsRegular(HeaderValue),
    NotCors,
}

impl<G, L> Cors<G, L> {
    pub fn builder() -> CorsConfiguration {
        CorsConfiguration::default()
    }

    /// Check a request for CORS support. Returns Ok(true) if the request is a
    /// valid preflight request, Ok(false) if it isn't. If the request appears
    /// to be a preflight but is invalid, a CorsRejection is issued.
    fn check_request(&self, request: &HttpRequest) -> Result<RequestType, CorsRejection> {
        // Only CORS if origin header is included
        let headers = request.headers();
        if let Some(origin) = headers.get(hyper::header::ORIGIN) {
            // Check origin
            if !self.is_origin_allowed(origin) {
                return Err(CorsRejection::Origin);
            }

            // Non-OPTIONS requests are not preflight
            if request.method() != Method::OPTIONS {
                return Ok(RequestType::CorsRegular(origin.clone()));
            }

            // Check preflight request
            // Must have Access-Control-Request-Method header
            if let Some(requested_method) = headers.get(header::ACCESS_CONTROL_REQUEST_METHOD) {
                if !self.is_method_allowed(requested_method) {
                    return Err(CorsRejection::Method);
                }
            } else {
                return Err(CorsRejection::Method);
            }

            // Access-Control-Request-Headers isnt required
            if let Some(requested_headers) = headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
                for header in requested_headers.to_str().unwrap_or_default().split(',') {
                    if !self.is_header_allowed(header) {
                        return Err(CorsRejection::Header);
                    }
                }
            }

            // Validated Preflight
            Ok(RequestType::CorsPreflight(origin.clone()))
        } else {
            // Not Cors
            Ok(RequestType::NotCors)
        }
    }

    fn is_method_allowed(&self, method: &hyper::header::HeaderValue) -> bool {
        match self.config.allow_methods {
            Allowable::Any => true,
            Allowable::Bounded(Bounded::Some(ref allowed_methods)) => {
                if let Ok(method) = Method::from_bytes(method.as_bytes()) {
                    allowed_methods.contains(&method)
                } else {
                    false
                }
            }
            Allowable::Bounded(Bounded::None) => false,
        }
    }

    fn is_header_allowed(&self, header: &str) -> bool {
        self.config.allow_headers.includes(&header)
        // match self.allow_headers {
        //     Bounded::Some(allowed) => allowed.iter().any(|h| h.eq(header)),
        //     Bounded::None => false,
        // }
    }

    fn is_origin_allowed(&self, origin: &hyper::header::HeaderValue) -> bool {
        match self.config.allow_origins {
            Allowable::Any => true,
            Allowable::Bounded(Bounded::Some(ref allowed_origins)) => allowed_origins
                .iter()
                .any(|o| o.eq(origin.to_str().unwrap_or_default())),
            Allowable::Bounded(Bounded::None) => false,
        }
    }

    // TODO: Performance, preallocate header map size?
    // fn preflight_header_capacity(&self) -> usize {

    fn preflight_response(&self, origin: HeaderValue) -> HttpResponse {
        let mut resp = HttpResponse::new(HttpBody::empty());

        let mut headers = if self.config.allow_credentials {
            let mut h = HeaderMap::with_capacity(2);
            h.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
            h
        } else {
            HeaderMap::with_capacity(1)
        };

        // Allow Origin
        // TODO: handle any origin response: "*"? CORS requires that the sender
        // sender origin be returned on a credentialed request, so we'd need to
        // see if the request had credentials...
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);

        // Exposed Headers
        if let Bounded::Some(ref exposed) = self.config.expose_headers {
            if let Some(exposed) = join_header_names(exposed.as_slice()) {
                headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, exposed);
            }
        }

        // header::ACCESS_CONTROL_MAX_AGE

        if self.config.allow_credentials {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }

        // Allowed Methods
        match self.config.allow_methods {
            Allowable::Any => {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    HeaderValue::from_static(LIST_ALL_METHODS),
                );
            }
            Allowable::Bounded(Bounded::Some(ref methods)) => {
                if !methods.is_empty() {
                    let methods = methods
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<&str>>()
                        .join(", ");

                    if let Ok(methods) = HeaderValue::from_str(&methods) {
                        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, methods);
                    }
                }
            }
            Allowable::Bounded(Bounded::None) => {}
        };

        // Allowed Headers
        if let Bounded::Some(ref allow_headers) = self.config.allow_headers {
            if let Some(allow_headers) = join_header_names(allow_headers.as_slice()) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allow_headers);
            }
        }
        *resp.headers_mut() = headers;
        resp
    }
}

fn join_header_names(names: &[HeaderName]) -> Option<HeaderValue> {
    let joined = names
        .iter()
        .map(|name| name.as_str())
        .collect::<Vec<&str>>()
        .join(", ");

    HeaderValue::from_str(&joined).ok()
}

enum Allowable<T> {
    Any,
    Bounded(Bounded<T>),
}

impl<T> Allowable<T>
where
    T: PartialEq,
{
    fn none() -> Self {
        Allowable::Bounded(Bounded::None)
    }

    // fn some(options: Vec<T>) -> Self {
    //     Allowable::Bounded(Bounded::Some(options))
    // }

    fn add(&mut self, item: T) {
        match self {
            Allowable::Any => {}
            Allowable::Bounded(b) => b.add(item),
        }
    }

    // fn contains(&self, item: &T) -> bool {
    //     match *self {
    //         Allowable::Any => true,
    //         Allowable::Bounded(ref b) => b.contains(item),
    //     }
    // }

    // fn includes<U>(&self, item: &U) -> bool
    // where
    //     U: PartialEq<T>,
    // {
    //     match *self {
    //         Allowable::Any => true,
    //         Allowable::Bounded(ref b) => b.includes(item),
    //     }
    // }
}

enum Bounded<T> {
    Some(Vec<T>),
    None,
}

impl<T> Bounded<T>
where
    T: PartialEq,
{
    fn add(&mut self, item: T) {
        match *self {
            Bounded::Some(ref mut v) => {
                if !v.contains(&item) {
                    v.push(item);
                }
            }
            Bounded::None => {
                *self = Bounded::Some(vec![item]);
            }
        }
    }

    // fn contains(&self, item: &T) -> bool {
    //     match *self {
    //         Bounded::None => false,
    //         Bounded::Some(ref v) => v.contains(item),
    //     }
    // }

    fn includes<U>(&self, item: &U) -> bool
    where
        U: PartialEq<T>,
    {
        match *self {
            Bounded::None => false,
            Bounded::Some(ref v) => v.iter().any(|x| item.eq(x)),
        }
    }
}

enum CorsRejection {
    Origin,
    Method,
    Header,
}

impl std::fmt::Display for CorsRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CorsRejection::Origin => write!(f, "Origin not allowed"),
            CorsRejection::Method => write!(f, "Method not allowed"),
            CorsRejection::Header => write!(f, "Header not allowed"),
        }
    }
}

impl From<CorsRejection> for Response {
    fn from(r: CorsRejection) -> Self {
        match r {
            CorsRejection::Origin => Response::Text(Status::FORBIDDEN, r.to_string()),
            CorsRejection::Method => Response::Text(Status::METHOD_NOT_ALLOWED, r.to_string()),
            CorsRejection::Header => Response::Text(Status::FORBIDDEN, r.to_string()),
        }
    }
}

impl From<CorsRejection> for HttpResponse {
    fn from(r: CorsRejection) -> Self {
        HttpResponse::from(Response::from(r))
    }
}

impl<G, L> ToMiddleware<G, L> for CorsConfiguration
where
    G: Send + Sync + 'static,
    L: Send + Sync + 'static,
{
    fn wrap(self, next: Arc<dyn Middleware<G, L>>) -> Arc<dyn Middleware<G, L>> {
        Arc::new(Cors { config: self, next })
    }
}

#[async_trait]
impl<GlobalCtx, LocalCtx> Middleware<GlobalCtx, LocalCtx> for Cors<GlobalCtx, LocalCtx>
where
    GlobalCtx: Send + Sync + 'static,
    LocalCtx: Send + Sync + 'static,
{
    async fn call(&self, context: &mut RequestContext<GlobalCtx, LocalCtx>) -> HttpResponse {
        match self.check_request(&context.request) {
            // Preflight request
            Ok(RequestType::CorsPreflight(origin)) => self.preflight_response(origin),
            // Regular Cors Request, Make sure to add origin header
            Ok(RequestType::CorsRegular(origin)) => {
                let mut resp = self.next.call(context).await;
                resp.headers_mut()
                    .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
                resp
            }
            // Regular request (do nothing)
            Ok(RequestType::NotCors) => self.next.call(context).await,
            // Cors rejection
            Err(rejection) => {
                #[cfg(feature = "tracing")]
                tracing::warn!("Issuing CORS rejection: {}", rejection);

                HttpResponse::from(rejection)
            }
        }
    }
}

// TODO: Tests
