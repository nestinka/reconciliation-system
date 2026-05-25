use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use recon_store::StoreError;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

#[allow(non_snake_case)]
impl ApiError {
    pub fn Unauthorized() -> Self {
        Self { status: StatusCode::UNAUTHORIZED, code: "unauthorized", message: "unauthorized".into(), details: None }
    }
    pub fn Forbidden() -> Self {
        Self { status: StatusCode::FORBIDDEN, code: "forbidden", message: "forbidden".into(), details: None }
    }
    pub fn NotFound() -> Self {
        Self { status: StatusCode::NOT_FOUND, code: "not_found", message: "not found".into(), details: None }
    }
    pub fn Conflict() -> Self {
        Self { status: StatusCode::CONFLICT, code: "conflict", message: "conflict".into(), details: None }
    }
    pub fn TooManyRequests() -> Self {
        Self { status: StatusCode::TOO_MANY_REQUESTS, code: "too_many_requests", message: "too many requests".into(), details: None }
    }
    pub fn BadRequest() -> Self {
        Self { status: StatusCode::BAD_REQUEST, code: "bad_request", message: "bad request".into(), details: None }
    }
    pub fn unauthorized(m: impl Into<String>) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, code: "unauthorized", message: m.into(), details: None }
    }
    pub fn with_details(status: StatusCode, code: &'static str, message: impl Into<String>, details: serde_json::Value) -> Self {
        Self { status, code, message: message.into(), details: Some(details) }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
                message: "not found".into(),
                details: None,
            },
            StoreError::Conflict(m) => ApiError {
                status: StatusCode::CONFLICT,
                code: "conflict",
                message: m,
                details: None,
            },
            StoreError::Forbidden(m) => ApiError {
                status: StatusCode::FORBIDDEN,
                code: "forbidden",
                message: m,
                details: None,
            },
            StoreError::DuplicateRefs(refs) => ApiError {
                status: StatusCode::CONFLICT,
                code: "duplicate",
                message: "duplicate transaction references".into(),
                details: Some(json!({ "refs": refs })),
            },
            StoreError::Db(_) | StoreError::Json(_) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal",
                message: "internal error".into(),
                details: None,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut err = json!({ "code": self.code, "message": self.message });
        if let Some(serde_json::Value::Object(map)) = self.details {
            if let serde_json::Value::Object(target) = &mut err {
                for (k, v) in map {
                    if k != "code" && k != "message" {
                        target.insert(k, v);
                    }
                }
            }
        }
        (self.status, Json(json!({ "error": err }))).into_response()
    }
}
