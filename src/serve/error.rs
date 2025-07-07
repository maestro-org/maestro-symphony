use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::fmt::Debug;
use thiserror::Error;
use tracing::error;

use crate::DecodingError;

#[derive(Error, Debug)]
pub enum ServeError {
    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Decoding error: {0}")]
    Decoding(#[from] DecodingError),

    #[error("Unable to find user requested data")]
    NotFound,

    #[error("Users request/query was malformed: {0}")]
    MalformedRequest(String),

    #[error("symphony error: {0}")]
    Symphony(#[from] crate::Error),

    #[error("unexpected missing data: {0}")]
    UnexpectedMissingData(String),
}

impl ServeError {
    pub fn malformed_request(str: impl ToString) -> Self {
        ServeError::MalformedRequest(str.to_string())
    }

    pub fn internal(str: impl ToString) -> Self {
        ServeError::Internal(str.to_string())
    }

    pub fn missing_data(x: impl Debug) -> Self {
        ServeError::UnexpectedMissingData(format!("{x:?}"))
    }
}

impl IntoResponse for ServeError {
    fn into_response(self) -> Response {
        let (status, string) = match self {
            ServeError::NotFound => (
                StatusCode::NOT_FOUND,
                format!("unable to find requested data"),
            ),
            ServeError::MalformedRequest(e) => (
                StatusCode::BAD_REQUEST,
                format!("unable to parse request parameters: {e}"),
            ),
            _ => {
                error!("internal server error: {}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("internal server error"),
                )
            }
        };

        (
            status,
            Json(json!({
                "error": string
            })),
        )
            .into_response()
    }
}
