//! Handler error type, rendered as a small standalone HTML page.

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use maud::{DOCTYPE, html};

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Forbidden(&'static str),
    BadRequest(String),
    Internal(anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError::Internal(e.into())
    }
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "Not found",
                "That page doesn't exist, or you don't have access to it.".into(),
            ),
            AppError::Forbidden(why) => (StatusCode::FORBIDDEN, "Not allowed", (*why).to_string()),
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "Something's not right",
                msg.clone(),
            ),
            AppError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong",
                    format!("{e:#}"),
                )
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, detail) = self.parts();
        let page = html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) " · LocalApp" }
                    style { (ERROR_CSS) }
                }
                body {
                    main {
                        h1 { (title) }
                        p { (detail) }
                        p { a href="javascript:history.back()" { "← Go back" } }
                    }
                }
            }
        };
        (status, Html(page.into_string())).into_response()
    }
}

/// Self-contained styles, so error pages work even if the stylesheet can't load.
const ERROR_CSS: &str = concat!(
    "body{font:15px/1.5 system-ui,sans-serif;margin:0;padding:3rem 1rem;",
    "background:#f6f5f2;color:#1f1e1c}",
    "main{max-width:34rem;margin:auto;background:#fff;border:1px solid #e2e0da;",
    "border-radius:12px;padding:1.5rem 2rem}",
    "h1{font-size:1.3rem;margin:.2rem 0 .6rem}a{color:#2a78d6}",
    "@media (prefers-color-scheme:dark){body{background:#161615;color:#e9e7e2}",
    "main{background:#1f1f1d;border-color:#34332f}a{color:#5b9cf0}}",
);
