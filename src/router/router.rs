use std::sync::Arc;

use axum::{Router, response::IntoResponse, routing::get};
use conduwuit::Error;
use conduwuit_api::router::{state, state::Guard};
use conduwuit_service::Services;
use http::{StatusCode, Uri};
use ruma::api::client::error::ErrorKind;

pub(crate) fn build(services: &Arc<Services>) -> (Router, Guard) {
	let router = Router::<state::State>::new();
	let (state, guard) = state::create(services.clone());
	let router = conduwuit_api::router::build(router, &services.server)
		.route("/", get(it_works))
		.fallback(not_found)
		.with_state(state);

	(router, guard)
}

async fn not_found(_uri: Uri) -> impl IntoResponse {
	Error::Request(ErrorKind::Unrecognized, "Not Found".into(), StatusCode::NOT_FOUND)
}

async fn it_works() -> &'static str { "hewwo from conduwuit woof!" }
