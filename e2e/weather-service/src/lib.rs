//! An in-memory weather service: the weather fixture's operations, generated
//! by build.rs from the `weather-model` package, implemented here.
//!
//! Test hooks: `x-fail-times: N` with `x-fail-key: K` answers the first N
//! requests under that key with a throttling error. It is a handler, not part
//! of any operation, the way a real service's throttling would be.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crucible::handlers::{ExceptionTranslator, ServiceLog};
use crucible::{Crucible, Handler, InvokeProtocol, Job, JsonProtocol, WithContext};

pub mod api {
    crucible::include_service!();
}

use api::errors::{ConflictError, NotFoundError, ThrottlingError};
use api::model::{
    CitySummary, CreateCityInput, CreateCityOutput, GetCityInput, GetCityOutput, ListCitiesInput,
    ListCitiesOutput, Sky, StatusInput, StatusOutput,
};

#[derive(Default)]
struct State {
    cities: BTreeMap<String, CitySummary>,
    /// Idempotency receipts: client token → city id.
    tokens: BTreeMap<String, String>,
    next_id: u32,
}

#[derive(Default)]
pub struct Weather {
    state: Mutex<State>,
}

impl api::PingOperation for Weather {
    async fn ping(&self) -> crucible::Result<()> {
        Ok(())
    }
}

impl api::StatusOperation for Weather {
    async fn status(&self, _: StatusInput) -> crucible::Result<StatusOutput> {
        Ok(StatusOutput {})
    }
}

impl api::CreateCityOperation for Weather {
    async fn create_city(&self, input: CreateCityInput) -> crucible::Result<CreateCityOutput> {
        let mut state = self.state.lock().unwrap();
        if let Some(id) = input
            .client_token
            .as_ref()
            .and_then(|token| state.tokens.get(token))
        {
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
            // The expected revision stands in for a population, so a test can
            // see a header member arrive.
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
}

impl api::GetCityOperation for Weather {
    async fn get_city(&self, input: GetCityInput) -> crucible::Result<GetCityOutput> {
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
            // An operation can model the request-id header itself; the
            // framework's own id is still on the context.
            request_id: Some(if self.context().request_id().is_empty() {
                "no context".to_owned()
            } else {
                "req-from-server".to_owned()
            }),
        })
    }
}

impl api::ListCitiesOperation for Weather {
    async fn list_cities(&self, input: ListCitiesInput) -> crucible::Result<ListCitiesOutput> {
        let state = self.state.lock().unwrap();
        let tags = input.tags.unwrap_or_default();
        let matching: Vec<&CitySummary> = state
            .cities
            .values()
            .filter(|city| tags.iter().all(|tag| tag == &city.name))
            .filter(|city| {
                input
                    .next_token
                    .as_ref()
                    .is_none_or(|token| city.id > *token)
            })
            .collect();
        let size = input.max_results.unwrap_or(100) as usize;
        let page: Vec<CitySummary> = matching
            .iter()
            .take(size)
            .map(|city| (*city).clone())
            .collect();
        let next_token = (matching.len() > size)
            .then(|| page.last().map(|city| city.id.clone()))
            .flatten();
        Ok(ListCitiesOutput {
            cities: page,
            next_token,
        })
    }
}

/// The throttling test hook; see the module docs.
#[derive(Default)]
struct FailTimes {
    seen: Mutex<BTreeMap<String, u32>>,
}

#[crucible::async_trait]
impl Handler for FailTimes {
    async fn before(&self, job: &mut Job) -> crucible::Result<()> {
        let header = |name: &str| {
            job.request()
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (Some(times), Some(key)) = (header("x-fail-times"), header("x-fail-key")) else {
            return Ok(());
        };
        let mut seen = self.seen.lock().unwrap();
        let count = seen.entry(key).or_default();
        if *count < times.parse().unwrap_or(0) {
            *count += 1;
            return Err(ThrottlingError {
                message: "slow down".to_owned(),
                retry_after: Some(0),
            }
            .into());
        }
        Ok(())
    }
}

/// The service: every operation on one `Weather`, behind the default chain
/// plus the test hook.
pub fn service() -> Crucible {
    Crucible::builder(api::operations(Arc::new(Weather::default())))
        .handler(ServiceLog::errors_only())
        .handler(ExceptionTranslator::new())
        .handler(FailTimes::default())
        .handler(JsonProtocol)
        .handler(InvokeProtocol)
        .build()
        .expect("the weather service builds")
}
