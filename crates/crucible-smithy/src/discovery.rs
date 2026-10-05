use crate::ast::{self, Kind};
use crate::ir::Operation;
use crate::lower::Lowerer;
use crate::{Feature, ShapeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointDiscovery {
    pub operation: ShapeId,
    pub error: Option<ShapeId>,
    pub operation_member: Option<String>,
    pub identifiers_member: Option<String>,
    pub endpoints_member: String,
    pub address_member: String,
    pub cache_period_member: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredEndpoint {
    pub required: bool,
    pub identifiers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiscovery {
    operation: ShapeId,
    error: Option<ShapeId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDiscovered {
    #[serde(default)]
    required: bool,
}

fn aws(name: &str) -> ShapeId {
    ShapeId::new("aws.api", name)
}
fn string_shape(shape: &ast::Shape) -> bool {
    matches!(shape.kind, Kind::String | Kind::Enum)
}

impl Lowerer<'_> {
    pub(crate) fn discovery(
        &mut self,
        service: &ShapeId,
        shape: &ast::Shape,
        operations: &mut [Operation],
    ) -> Option<EndpointDiscovery> {
        let config_id = aws("clientEndpointDiscovery");
        let config = shape.traits.get(&config_id).and_then(|value| {
            self.require(service, Feature::EndpointDiscovery, config_id.clone());
            match serde_json::from_value::<RawDiscovery>(value.clone()) {
                Ok(config) => Some(config),
                Err(error) => {
                    self.diagnostics
                        .push(service, format!("invalid @{config_id}: {error}"));
                    None
                }
            }
        });
        for operation in operations.iter_mut() {
            let trait_id = aws("clientDiscoveredEndpoint");
            let Some(value) = self.model[&operation.id].traits.get(&trait_id) else {
                continue;
            };
            self.require(&operation.id, Feature::EndpointDiscovery, trait_id.clone());
            let required = match serde_json::from_value::<RawDiscovered>(value.clone()) {
                Ok(config) => config.required,
                Err(error) => {
                    self.diagnostics
                        .push(&operation.id, format!("invalid @{trait_id}: {error}"));
                    false
                }
            };
            if let Some(config) = &config {
                if config.operation == operation.id {
                    self.diagnostics.push(
                        &operation.id,
                        "endpoint discovery operation cannot itself use endpoint discovery",
                    );
                }
                if config
                    .error
                    .as_ref()
                    .is_some_and(|error| !operation.errors.contains(error))
                {
                    self.diagnostics.push(
                        &operation.id,
                        "endpoint expiry error must be bound to every discovered operation",
                    );
                }
            } else {
                self.diagnostics.push(
                    &operation.id,
                    "@clientDiscoveredEndpoint requires service @clientEndpointDiscovery",
                );
            }
            let mut identifiers = Vec::new();
            if let Some(input) = operation.input.as_ref().and_then(|id| self.model.get(id)) {
                for (name, member) in &input.members {
                    if member
                        .traits
                        .contains_key(&aws("clientEndpointDiscoveryId"))
                    {
                        if !member
                            .traits
                            .contains_key(&ShapeId::new("smithy.api", "required"))
                            || !self.model.get(&member.target).is_some_and(string_shape)
                        {
                            self.diagnostics.push(
                                &operation.input.as_ref().unwrap().with_member(name),
                                "endpoint discovery identifiers must be required strings",
                            );
                        }
                        identifiers.push(name.clone());
                    }
                }
            }
            operation.discovered_endpoint = Some(DiscoveredEndpoint {
                required,
                identifiers,
            });
        }
        let config = config?;
        let Some(operation) = operations
            .iter()
            .find(|operation| operation.id == config.operation)
        else {
            self.diagnostics.push(
                service,
                "endpoint discovery operation must be bound to the service",
            );
            return None;
        };
        let mut operation_member = None;
        let mut identifiers_member = None;
        if let Some(input) = operation.input.as_ref().and_then(|id| self.model.get(id)) {
            for (name, member) in &input.members {
                let target = self.model.get(&member.target);
                match name.as_str() {
                    "Operation" if target.is_some_and(string_shape)=>operation_member=Some(name.clone()),
                    "Identifiers" if target.is_some_and(|shape|shape.kind==Kind::Map && shape.key.as_ref().and_then(|m|self.model.get(&m.target)).is_some_and(string_shape) && shape.value.as_ref().and_then(|m|self.model.get(&m.target)).is_some_and(string_shape))=>identifiers_member=Some(name.clone()),
                    _=>self.diagnostics.push(&operation.input.as_ref().unwrap().with_member(name),"discovery input permits only Operation:string and Identifiers:map<string,string>"),
                }
            }
        }
        let endpoint = operation
            .output
            .as_ref()
            .and_then(|id| self.model.get(id))
            .and_then(|shape| shape.members.get("Endpoints"))
            .and_then(|member| self.model.get(&member.target))
            .filter(|shape| shape.kind == Kind::List)
            .and_then(|shape| shape.member.as_ref())
            .and_then(|member| self.model.get(&member.target))
            .filter(|shape| shape.kind == Kind::Structure);
        if !endpoint.is_some_and(|shape| {
            shape
                .members
                .get("Address")
                .and_then(|m| self.model.get(&m.target))
                .is_some_and(string_shape)
                && shape
                    .members
                    .get("CachePeriodInMinutes")
                    .and_then(|m| self.model.get(&m.target))
                    .is_some_and(|s| s.kind == Kind::Long)
        }) {
            self.diagnostics.push(&config.operation,"discovery output must contain Endpoints:list<structure> with Address:string and CachePeriodInMinutes:long");
        }
        Some(EndpointDiscovery {
            operation: config.operation,
            error: config.error,
            operation_member,
            identifiers_member,
            endpoints_member: "Endpoints".into(),
            address_member: "Address".into(),
            cache_period_member: "CachePeriodInMinutes".into(),
        })
    }
}
