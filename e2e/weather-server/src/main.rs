//! An in-memory weather service on the crate crucible generates from the
//! weather fixture. tools/test-e2e.sh drives it with the generated
//! TypeScript client over real HTTP.
//!
//! Prints `listening on <address>` once it accepts connections. Test hooks:
//! `x-fail-times: N` with `x-fail-key: K` answers the first N calls under
//! that key with a throttling error.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use weather_api::Context;
use weather_api::api::{CityApi, Reject, WeatherApi};
use weather_api::errors::{
    ConflictError, CreateCityError, GetCityError, ListCitiesError, NotFoundError, PingError,
    ThrottlingError,
};
use weather_api::models::{
    CitySummary, CreateCityInput, CreateCityOutput, GetCityInput, GetCityOutput, ListCitiesInput,
    ListCitiesOutput, Sky,
};

#[derive(Default)]
struct State {
    cities: BTreeMap<String, CitySummary>,
    tags: BTreeMap<String, Vec<String>>,
    /// Idempotency receipts: client token → city id.
    tokens: BTreeMap<String, String>,
    failures: BTreeMap<String, u32>,
    next_id: u32,
}

#[derive(Clone, Default)]
struct Weather {
    state: Arc<Mutex<State>>,
}

impl Weather {
    /// The throttling test hook; see the module docs.
    fn throttle(&self, ctx: &Context) -> Result<(), ThrottlingError> {
        let header = |name: &str| ctx.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let (Some(times), Some(key)) = (header("x-fail-times"), header("x-fail-key")) else {
            return Ok(());
        };
        let times: u32 = times.parse().unwrap_or(0);
        let mut state = self.state.lock().unwrap();
        let seen = state.failures.entry(key).or_default();
        if *seen < times {
            *seen += 1;
            return Err(ThrottlingError {
                message: "slow down".to_owned(),
                retry_after: Some(0),
            });
        }
        Ok(())
    }
}

impl Reject for Weather {}

impl WeatherApi for Weather {
    async fn ping(&self, ctx: &Context) -> Result<(), PingError> {
        self.throttle(ctx)?;
        Ok(())
    }
}

impl CityApi for Weather {
    async fn create_city(&self, ctx: &Context, input: CreateCityInput) -> Result<CreateCityOutput, CreateCityError> {
        self.throttle(ctx)?;
        let mut state = self.state.lock().unwrap();
        if let Some(id) = input.client_token.as_ref().and_then(|token| state.tokens.get(token)) {
            return Ok(CreateCityOutput {
                city: state.cities[id].clone(),
            });
        }
        if state.cities.values().any(|city| city.name == input.name) {
            return Err(ConflictError {
                message: format!("a city named {} exists", input.name),
            }
            .into());
        }
        state.next_id += 1;
        let id = format!("c-{:08}", state.next_id);
        let now = chrono::Utc::now();
        let city = CitySummary {
            id: id.clone(),
            name: input.name,
            // The expected revision stands in for a population, so a test
            // can see a header member arrive.
            population: input.expected_revision.unwrap_or(0),
            updated_at: Some(now),
            created_at: Some(now),
        };
        state.cities.insert(id.clone(), city.clone());
        if let Some(token) = input.client_token {
            state.tokens.insert(token, id);
        }
        Ok(CreateCityOutput { city })
    }

    async fn get_city(&self, ctx: &Context, input: GetCityInput) -> Result<GetCityOutput, GetCityError> {
        self.throttle(ctx)?;
        let state = self.state.lock().unwrap();
        let Some(city) = state.cities.get(&input.city_id) else {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        };
        // Echoes the query timestamp, so a test can see it arrive intact.
        let forecast = input
            .at
            .map(|at| BTreeMap::from([(Sky::Clear, serde_json::json!({ "at": at.to_rfc3339() }))]));
        Ok(GetCityOutput {
            city: city.clone(),
            forecast,
            request_id: Some("req-from-server".to_owned()),
        })
    }

    async fn list_cities(&self, ctx: &Context, input: ListCitiesInput) -> Result<ListCitiesOutput, ListCitiesError> {
        self.throttle(ctx)?;
        let state = self.state.lock().unwrap();
        let tags = input.tags.unwrap_or_default();
        let matching: Vec<&CitySummary> = state
            .cities
            .values()
            .filter(|city| {
                let held = state.tags.get(&city.id).cloned().unwrap_or_default();
                tags.iter().all(|tag| held.contains(tag) || tag == &city.name)
            })
            .filter(|city| input.next_token.as_ref().is_none_or(|token| city.id > *token))
            .collect();
        let size = input.max_results.unwrap_or(100) as usize;
        let page: Vec<CitySummary> = matching.iter().take(size).map(|city| (*city).clone()).collect();
        let next_token = (matching.len() > size).then(|| page.last().map(|city| city.id.clone())).flatten();
        Ok(ListCitiesOutput { cities: page, next_token })
    }
}

#[tokio::main]
async fn main() {
    let address = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:0".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await.expect("bind");
    println!("listening on {}", listener.local_addr().expect("address"));
    axum::serve(listener, weather_api::router(Weather::default())).await.expect("serve");
}
