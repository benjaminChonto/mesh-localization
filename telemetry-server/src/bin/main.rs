use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::{EncodeLabelSet, EncodeLabelValue};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;
use serde::Deserialize;

#[derive(Clone, Hash, PartialEq, Eq, Debug, EncodeLabelSet)]
struct RequestLabels {
    method: Method,
    path: String,
    id: String,
    num_neigbors: i32,
}

#[derive(Deserialize)]
struct UpdateRequest {
    id: String,
    num_neighbors: i32,
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
            id: "WEB".to_string(),
            num_neigbors: 0,
        })
        .inc();
    HttpResponse::Ok().body("Hello, World!")
}

#[post("/update")]
async fn update(
    esp_data: web::Json<UpdateRequest>,
    request_counter: web::Data<Family<RequestLabels, Counter>>,
) -> impl Responder {
    let id = &esp_data.id;
    let num_neighbors = &esp_data.num_neighbors;
    request_counter
        .get_ref()
        .get_or_create(&RequestLabels {
            method: Method::POST,
            path: "/update".to_string(),
            id: id.clone(),
            num_neigbors: *num_neighbors,
        })
        .inc();
    HttpResponse::Ok().body("Update successful!")
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
    let address = ("0.0.0.0", 8080);
    println!("listening on address: {:?}", address);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(request_counter.clone()))
            .app_data(registry.clone())
            .service(index)
            .service(update)
            .route("/metrics", web::get().to(metrics))
    })
    .bind(address)?
    .run()
    .await
}
