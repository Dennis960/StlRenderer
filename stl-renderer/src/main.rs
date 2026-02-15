mod math;
mod parser;
mod render;
mod server;

use actix_web::{web, App, HttpServer, middleware};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if std::env::args().any(|a| a == "healthcheck") {
        std::process::exit(0);
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    log::info!("Starting STL Renderer on {}", bind);

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::PayloadConfig::new(50 * 1024 * 1024))
            .service(server::index)
            .service(server::style_css)
            .service(server::app_js)
            .service(server::render_endpoint)
            .service(server::health)
    })
    .bind(&bind)?
    .run()
    .await
}
