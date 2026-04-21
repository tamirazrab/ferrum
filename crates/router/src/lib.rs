pub mod auth;
pub mod config;
pub mod routes;
pub mod types;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{
    web::{self, scope},
    App, HttpResponse,
};

use crate::auth::ApiKeyAuth;
use crate::routes::{depth, klines, order, readiness, tickers, trade, user};
use crate::types::app::AppState;

fn exchange_cors() -> Cors {
    const MAX_AGE: usize = 3600;
    match std::env::var("CORS_ALLOWED_ORIGIN") {
        Ok(ref origin) if !origin.is_empty() => {
            Cors::default()
                .allowed_origin(origin.as_str())
                .allow_any_header()
                .allow_any_method()
                .max_age(MAX_AGE)
        }
        _ => Cors::default()
            .allow_any_origin()
            .allow_any_header()
            .allow_any_method()
            .max_age(MAX_AGE),
    }
}

pub fn build_app(
    app_state: web::Data<AppState>,
    api_key: String,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let governor_conf = GovernorConfigBuilder::default()
        .seconds_per_request(1)
        .burst_size(60)
        .finish()
        .expect("failed to build governor config");

    App::new()
        .wrap(Governor::new(&governor_conf))
        .wrap(exchange_cors())
        .wrap(ApiKeyAuth::new(api_key))
        .service(
            scope("/api/v1")
                .app_data(app_state)
                .service(web::scope("/health").route("", web::get().to(HttpResponse::Ok)))
                .service(
                    web::scope("/ready").route("", web::get().to(readiness::get_ready)),
                )
                .service(web::scope("/users").route("", web::post().to(user::create_user)))
                .service(web::scope("/depth").route("", web::get().to(depth::get_depth)))
                .service(web::scope("/trades").route("", web::get().to(trade::get_trades)))
                .service(web::scope("/klines").route("", web::get().to(klines::get_klines)))
                .service(
                    web::scope("/tickers").route("", web::get().to(tickers::get_tickers)),
                )
                .service(
                    web::scope("/order")
                        .route("", web::get().to(order::get_open_order))
                        .route("", web::post().to(order::execute_order))
                        .route("", web::delete().to(order::cancel_order)),
                )
                .service(
                    web::scope("/orders")
                        .route("", web::get().to(order::get_open_orders))
                        .route("", web::delete().to(order::cancel_all_orders)),
                ),
        )
}
