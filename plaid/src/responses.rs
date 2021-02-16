use hyper::header::HeaderValue;

use crate::prelude::*;

/// Shortcuts for generating [`Response`]s
pub mod respond {

    use super::*;

    /// Shortcut for Response::Empty(Status::OK)
    pub fn ok() -> Response {
        Response::Empty(Status::OK)
    }

    pub fn error() -> Response {
        Response::Empty(Status::INTERNAL_SERVER_ERROR)
    }

    pub fn method_not_allowed() -> Response {
        Response::Empty(Status::METHOD_NOT_ALLOWED)
    }

    pub fn not_found() -> Response {
        Response::Empty(Status::NOT_FOUND)
    }

    pub fn forbidden() -> Response {
        Response::Empty(Status::FORBIDDEN)
    }

    #[derive(serde::Serialize)]
    struct MultipleChoices {
        message: &'static str,
        options: Vec<String>,
    }
    pub fn multiple_choices(choices: &[String]) -> Result<Response, ResponseError> {
        json(
            Status::MULTIPLE_CHOICES,
            &MultipleChoices {
                message: "Multiple choices were found for the request target",
                options: choices.to_vec(),
            },
        )
    }

    pub fn payload_too_large(max_bytes: usize) -> Response {
        let mut resp = HttpResponse::new(HttpBody::from(format!(
            "Payload too large. Expected at most {} bytes",
            max_bytes
        )));
        *resp.status_mut() = Status::PAYLOAD_TOO_LARGE;
        // resp.headers_mut().insert(HeaderName::from_static("content-length-limit"), val)
        Response::Custom(resp)
    }

    pub fn status(status: Status) -> Response {
        Response::Empty(status)
    }

    pub fn json<T: serde::Serialize>(status: Status, body: &T) -> Result<Response, ResponseError> {
        let json = serde_json::to_string(body).map_err(ResponseError::SerializeJson)?;
        if json.is_empty() {
            Ok(Response::Empty(status))
        } else {
            Ok(Response::Json(status, json))
        }
    }

    // &[u8] instead? But how to stop from copying?
    pub fn bytes(
        status: Status,
        body: Vec<u8>,
        calculate_md5: bool,
    ) -> Result<Response, ResponseError> {
        if body.is_empty() {
            Ok(Response::Empty(status))
        } else {
            let md5 = if calculate_md5 {
                let hash = md5::compute(&body);
                Some(base64::encode_config(hash.as_ref(), base64::STANDARD))
            } else {
                None
            };
            Ok(Response::Bytes { status, body, md5 })
        }
    }

    pub fn unauthorized() -> Response {
        let mut resp = HttpResponse::new(HttpBody::empty());
        *resp.status_mut() = Status::UNAUTHORIZED;
        resp.headers_mut().insert(
            hyper::header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer"),
        );
        Response::Custom(resp)
    }

    /// Respond to a non-CORS OPTIONS request. Serialization of the methods
    /// COULD technically fail here, but this is an internal method, so we
    /// aren't going to expose the error to users to handle and the pattern in
    /// router section of our service will be simple if this handles errors
    /// internally.
    pub(crate) fn options(options: &[Method]) -> Response {
        let allow_methods = options
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<&str>>()
            .join(", ");

        let allow_header = HeaderValue::from_str(&allow_methods)
            .map_err(ResponseError::ToHeaderValue)
            .unwrap_or_else(|e| {
                #[cfg(feature = "tracing")]
                tracing::warn!("Failed to convert response to http: {}", e);
                HeaderValue::from_static("")
            });

        let mut resp = HttpResponse::new(HttpBody::empty());
        *resp.status_mut() = Status::NO_CONTENT;
        resp.headers_mut()
            .insert(hyper::header::ALLOW, allow_header);

        Response::Custom(resp)
    }
}

// TODO: docs
pub enum Response {
    Empty(Status),
    Text(Status, String),
    Bytes {
        status: Status,
        body: Vec<u8>,
        md5: Option<String>,
    },
    Json(Status, String),

    Custom(HttpResponse),
}

impl Response {
    pub fn status(&self) -> Status {
        match self {
            Response::Empty(ref s) => *s,
            Response::Text(ref s, _) => *s,
            Response::Bytes {
                status,
                body: _,
                md5: _,
            } => *status,
            Response::Json(ref s, _) => *s,
            Response::Custom(http) => http.status(),
        }
    }
}

#[derive(Debug)]
pub enum ResponseError {
    // Http(hyper::http::Error),
    SerializeJson(serde_json::Error),
    ToHeaderValue(hyper::header::InvalidHeaderValue),
}

impl std::fmt::Display for ResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseError::SerializeJson(ref e) => {
                write!(f, "Failed to serialize json body: {}", e)
            }
            ResponseError::ToHeaderValue(ref e) => {
                write!(f, "Failed to serialize header value: {}", e)
            }
        }
    }
}

const CONTENT_TYPE_TEXT: &str = "text/plain";
const CONTENT_TYPE_BYTES: &str = "application/octet-stream";
const CONTENT_TYPE_JSON: &str = "application/json";

impl From<Response> for HttpResponse {
    fn from(value: Response) -> Self {
        match value {
            Response::Empty(status) => hyper::Response::builder()
                .status(status)
                .body(hyper::Body::empty()),

            Response::Text(status, text) => hyper::Response::builder()
                .status(status)
                .header(
                    hyper::header::CONTENT_TYPE,
                    HeaderValue::from_static(CONTENT_TYPE_TEXT),
                )
                .body(hyper::Body::from(text)),

            Response::Bytes { status, body, md5 } => {
                let mut builder = hyper::Response::builder()
                    .status(status)
                    .header(
                        hyper::header::CONTENT_TYPE,
                        HeaderValue::from_static(CONTENT_TYPE_BYTES),
                    )
                    .header(hyper::header::CONTENT_LENGTH, body.len());

                if let Some(md5) = md5 {
                    match hyper::header::HeaderValue::from_str(&md5) {
                        Ok(md5) => {
                            builder = builder
                                .header(hyper::header::HeaderName::from_static("content-md5"), md5);
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to serialize byte response md5: {}", md5);
                        }
                    }
                }

                builder.body(hyper::Body::from(body))
            }
            Response::Json(status, json) => hyper::Response::builder()
                .status(status)
                .header(
                    hyper::header::CONTENT_TYPE,
                    HeaderValue::from_static(CONTENT_TYPE_JSON),
                )
                .body(hyper::Body::from(json)),

            Response::Custom(resp) => Ok(resp),
        }
        .unwrap_or_else(|e| {
            #[cfg(feature = "tracing")]
            tracing::error!("Failed to convert response to http: {}", e);

            HttpResponse::from(Response::Empty(Status::INTERNAL_SERVER_ERROR))
        })
    }
}
