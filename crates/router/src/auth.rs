use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use std::future::{ready, Future, Ready};
use std::pin::Pin;

/// Middleware that validates the `X-API-Key` header against a configured secret.
/// Requests to `/api/v1/health` and `/api/v1/ready` are exempt from authentication.
pub struct ApiKeyAuth {
    pub api_key: String,
}

impl ApiKeyAuth {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl<S, B> Transform<S, ServiceRequest> for ApiKeyAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = ApiKeyAuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ApiKeyAuthMiddleware {
            service,
            api_key: self.api_key.clone(),
        }))
    }
}

pub struct ApiKeyAuthMiddleware<S> {
    service: S,
    api_key: String,
}

impl<S, B> Service<ServiceRequest> for ApiKeyAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path();
        if path.starts_with("/api/v1/health") || path.starts_with("/api/v1/ready") {
            let fut = self.service.call(req);
            return Box::pin(async move {
                fut.await.map(ServiceResponse::map_into_left_body)
            });
        }

        let provided_key = req
            .headers()
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if provided_key != self.api_key {
            let response = HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "invalid or missing API key"}));
            return Box::pin(async move {
                Ok(req.into_response(response).map_into_right_body())
            });
        }

        let fut = self.service.call(req);
        Box::pin(async move {
            fut.await.map(ServiceResponse::map_into_left_body)
        })
    }
}
