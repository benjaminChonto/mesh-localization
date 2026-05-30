use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

#[derive(Clone, Hash, PartialEq, Eq, Debug, EncodeLabelSet)]
struct RequestLabels {
    method: Method,
    path: String,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug, EncodeLabelValue)]
enum Method {
    GET,
    POST,
    PUT,
    DELETE,
}

#[get("/")]
async fn index(request_counter: web::Data<Family<RequestLabels, Counter>>) -> impl Responder {
    request_counter
        .get_ref()
        .get_or_create(&RequestLabels {
            method: Method::GET,
            path: "/".to_string(),
        })
        .inc();
    HttpResponse::Ok().body("Hello, World!")
}

async fn metrics(registry: web::Data<Registry>) -> impl Responder {
    let mut buffer = String::new();
    encode(&mut buffer, &registry).unwrap();
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(buffer)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut registry = Registry::default();

    let request_counter = Family::<RequestLabels, Counter>::default();
    registry.register(
        "http_requests_total",
        "Total number of HTTP requests",
        request_counter.clone(),
    );

    // Wrap the registry in actix_web::web::Data so it can be cloned into the
    // server factory closure (Registry itself doesn't implement Clone).
    let registry = web::Data::new(registry);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(request_counter.clone()))
            .app_data(registry.clone())
            .service(index)
            .route("/metrics", web::get().to(metrics))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
