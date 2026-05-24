use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use recon_store::StoreError;
use serde_json::json;

// used by routes and auth extractor in a later task
#[allow(dead_code)]
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

// used by routes in a later task
#[allow(dead_code)]
impl ApiError {
    pub fn unauthorized(m: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: m.into(),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
                message: "not found".into(),
            },
            StoreError::Conflict(m) => ApiError {
                status: StatusCode::CONFLICT,
                code: "conflict",
                message: m,
            },
            StoreError::Forbidden(m) => ApiError {
                status: StatusCode::FORBIDDEN,
                code: "forbidden",
                message: m,
            },
            StoreError::Db(_) | StoreError::Json(_) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal",
                message: "internal error".into(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "code": self.code, "message": self.message } })),
        )
            .into_response()
    }
}
