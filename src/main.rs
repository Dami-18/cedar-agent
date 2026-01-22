extern crate core;
extern crate rocket;

use std::borrow::Borrow;
use std::process::ExitCode;

use log::{error, info};
use rocket::catchers;
use rocket::http::ContentType;
use rocket_cors::{AllowedHeaders, AllowedOrigins, CorsOptions};
use rocket_okapi::settings::UrlObject;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};

use crate::services::data::memory::MemoryDataStore;
use crate::services::data::DataStore;
use crate::services::policies::memory::MemoryPolicyStore;
use crate::services::policies::PolicyStore;
use crate::services::schema::memory::MemorySchemaStore;
use crate::services::schema::SchemaStore;
use crate::services::stats::memory::MemoryStatsStore;
use crate::services::stats::StatsStore;

mod authn;
mod common;
mod config;
mod errors;
mod logger;
mod routes;
mod schemas;
mod services;

#[rocket::main]
async fn main() -> ExitCode {
    let config = config::init();
    logger::init(&config);
    let server_config: rocket::figment::Figment = config.borrow().into();

    // Configure CORS
    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .allowed_methods(
            vec![
                rocket::http::Method::Get,
                rocket::http::Method::Post,
                rocket::http::Method::Put,
                rocket::http::Method::Patch,
                rocket::http::Method::Delete,
                rocket::http::Method::Options,
            ]
            .into_iter()
            .map(From::from)
            .collect(),
        )
        .allowed_headers(AllowedHeaders::some(&["Authorization", "Content-Type"]))
        .allow_credentials(true);

    let cors_fairing = match cors.to_cors() {
        Ok(fairing) => fairing,
        Err(err) => {
            error!("Failed to configure CORS: {}", err);
            return ExitCode::FAILURE;
        }
    };

    let launch_result = rocket::custom(server_config)
        .attach(cors_fairing)
        .attach(common::DefaultContentType::new(ContentType::JSON))
        .attach(services::schema::load_from_file::InitSchemaFairing)
        .attach(services::data::load_from_file::InitDataFairing)
        .attach(services::policies::load_from_file::InitPoliciesFairing)
        .manage(config)
        .manage(Box::new(MemoryPolicyStore::new()) as Box<dyn PolicyStore>)
        .manage(Box::new(MemoryDataStore::new()) as Box<dyn DataStore>)
        .manage(Box::new(MemorySchemaStore::new()) as Box<dyn SchemaStore>)
        .manage(Box::new(MemoryStatsStore::new()) as Box<dyn StatsStore>)
        .manage(cedar_policy::Authorizer::new())
        .register(
            "/",
            catchers![
                errors::catchers::handle_500,
                errors::catchers::handle_404,
                errors::catchers::handle_400,
            ],
        )
        .mount(
            "/v1",
            openapi_get_routes![
                routes::healthy,
                routes::policies::get_policies,
                routes::policies::get_policy,
                routes::policies::create_policy,
                routes::policies::update_policies,
                routes::policies::update_policy,
                routes::policies::delete_policy,
                routes::data::get_entities,
                routes::data::update_entities,
                routes::data::delete_entities,
                routes::data::add_single_data_entry,
                routes::data::update_single_data_entry,
                routes::data::delete_single_data_entry,
                routes::data::update_entity_attribute,
                routes::data::delete_entity_attribute,
                routes::data::patch_entity_attributes,
                routes::data::add_new_entity,
                routes::authorization::is_authorized,
                routes::schema::get_schema,
                routes::schema::update_schema,
                routes::schema::delete_schema,
                routes::schema::add_user_attribute,
                routes::schema::add_table_attribute,
                routes::schema::delete_user_attribute,
                routes::schema::delete_table_attribute,
                routes::schema::add_generic_attribute,
                routes::schema::delete_generic_attribute,
                routes::stats::get_stats,
                routes::stats::reset_stats,
            ],
        )
        .mount(
            "/swagger-ui/",
            make_swagger_ui(&SwaggerUIConfig {
                url: "../v1/openapi.json".to_owned(),
                ..Default::default()
            }),
        )
        .mount(
            "/rapidoc/",
            make_rapidoc(&RapiDocConfig {
                general: GeneralConfig {
                    spec_urls: vec![UrlObject::new("General", "../v1/openapi.json")],
                    ..Default::default()
                },
                hide_show: HideShowConfig {
                    allow_spec_url_load: false,
                    allow_spec_file_load: false,
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .launch()
        .await;
    match launch_result {
        Ok(_) => {
            info!("Cedar-Agent shut down gracefully.");
            ExitCode::SUCCESS
        }
        Err(err) => {
            error!("Cedar-Agent shut down with error: {}", err);
            ExitCode::FAILURE
        }
    }
}
