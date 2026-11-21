use axum::{Router, routing::get};

/// Construye el router HTTP principal de Roost.
pub fn router() -> Router {
    Router::new().route("/", get(index))
}

async fn index() -> &'static str {
    "Roost"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn root_returns_roost() {
        let app = router();

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.as_ref(), b"Roost");
    }
}
