#![forbid(unsafe_code)]

// TODO: doc async_trait required
#[macro_use]
extern crate async_trait;

// TODO: convert tests to use this macro
pub use plaid_macros::handler;

pub mod context;
mod handlers;
pub mod middleware;
pub mod responses;
mod routes;
mod server;

pub mod prelude {

    // Re-export hyper so consumers don't need to include it explicitly
    pub use hyper; //TODO: export http instead?
    pub type HttpResponse = hyper::Response<hyper::Body>;
    pub type HttpRequest = hyper::Request<hyper::Body>;
    pub type HttpBody = hyper::Body;
    pub type Method = hyper::Method;
    pub type Status = hyper::StatusCode;

    pub use super::context::RequestContext;
    pub use super::responses::{respond, Response};
    pub use super::server::{Server, ServerError};

    pub use super::handlers::*;
    // pub use super::router::handlers::{}

    pub use super::routes::{Parameter, RouteParameters, Router};

    pub use super::middleware::{Middleware, ToMiddleware};

    // pub type Result<Err> = std::result::Result<Response, Err>;
}

pub use prelude::*;
