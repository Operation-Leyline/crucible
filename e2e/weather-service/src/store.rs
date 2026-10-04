//! Where cities live. The service holds a `CityStore` and no data of its own:
//! a deployment passes one backed by a database, and tests and local runs
//! pass [`MemoryCities`].

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use crate::contract::model::CitySummary;

/// What a store needs to create a city; it assigns the id and timestamps.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCity {
    pub name: String,
    pub population: i64,
}

#[crucible::async_trait]
pub trait CityStore: Send + Sync + 'static {
    async fn get(&self, id: &str) -> crucible::Result<Option<CitySummary>>;

    /// Creates a city, or returns the one an earlier request with the same
    /// `token` created. A name already taken is [`NameTaken`].
    async fn create(&self, city: NewCity, token: Option<&str>) -> crucible::Result<CitySummary>;

    /// Cities after `after` in id order, at most `limit`, whose name is one
    /// of `names` when any are given.
    async fn list(
        &self,
        after: Option<&str>,
        limit: usize,
        names: &[String],
    ) -> crucible::Result<Vec<CitySummary>>;
}

/// The store's own error for a taken name; the service's exception
/// translator turns it into the modelled `ConflictError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameTaken(pub String);

impl fmt::Display for NameTaken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a city named {} exists", self.0)
    }
}

impl std::error::Error for NameTaken {}

/// Cities in memory: for tests and local runs, never for a deployment,
/// where each instance would hold different cities.
#[derive(Default)]
pub struct MemoryCities {
    state: Mutex<Memory>,
}

#[derive(Default)]
struct Memory {
    cities: BTreeMap<String, CitySummary>,
    /// Idempotency receipts: client token → city id.
    tokens: BTreeMap<String, String>,
    next_id: u32,
}

#[crucible::async_trait]
impl CityStore for MemoryCities {
    async fn get(&self, id: &str) -> crucible::Result<Option<CitySummary>> {
        Ok(self.state.lock().unwrap().cities.get(id).cloned())
    }

    async fn create(&self, city: NewCity, token: Option<&str>) -> crucible::Result<CitySummary> {
        let mut memory = self.state.lock().unwrap();
        if let Some(id) = token.and_then(|token| memory.tokens.get(token)) {
            return Ok(memory.cities[id].clone());
        }
        if memory
            .cities
            .values()
            .any(|existing| existing.name == city.name)
        {
            return Err(NameTaken(city.name).into());
        }
        memory.next_id += 1;
        let now = chrono::Utc::now();
        let created = CitySummary {
            id: format!("c-{:08}", memory.next_id),
            name: city.name,
            population: city.population,
            updated_at: Some(now),
            created_at: Some(now),
        };
        memory.cities.insert(created.id.clone(), created.clone());
        if let Some(token) = token {
            memory.tokens.insert(token.to_owned(), created.id.clone());
        }
        Ok(created)
    }

    async fn list(
        &self,
        after: Option<&str>,
        limit: usize,
        names: &[String],
    ) -> crucible::Result<Vec<CitySummary>> {
        let memory = self.state.lock().unwrap();
        Ok(memory
            .cities
            .values()
            .filter(|city| after.is_none_or(|after| city.id.as_str() > after))
            .filter(|city| names.is_empty() || names.contains(&city.name))
            .take(limit)
            .cloned()
            .collect())
    }
}
