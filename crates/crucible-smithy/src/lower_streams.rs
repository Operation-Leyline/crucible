use crate::ast::{self, Kind};
use crate::ir::{EventBinding, Operation};
use crate::lower::Lowerer;
use crate::{Feature, ShapeId};
use std::collections::BTreeSet;

fn trait_id(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

impl Lowerer<'_> {
    pub(crate) fn check_stream_shape(&mut self, id: &ShapeId, shape: &ast::Shape) {
        let streaming = shape.traits.contains_key(&trait_id("streaming"));
        if streaming {
            let feature = if shape.kind == Kind::Union {
                Feature::EventStreams
            } else {
                Feature::Streaming
            };
            self.require(id, feature, trait_id("streaming"));
            if self.protocol().is_rpc_v2() {
                // RPC v2 streams are event streams framed as application/vnd.amazon.eventstream;
                // no target implements that framing for these protocols yet.
                self.diagnostics.push(
                    id,
                    format!(
                        "@streaming is not supported with @{}",
                        self.protocol().trait_id()
                    ),
                );
            }
            if !matches!(shape.kind, Kind::Blob | Kind::Union) {
                self.diagnostics
                    .push(id, "@streaming must target a blob or union");
            }
            if shape.kind == Kind::Union {
                for (name, member) in &shape.members {
                    if !self
                        .model
                        .get(&member.target)
                        .is_some_and(|shape| shape.kind == Kind::Structure)
                    {
                        self.diagnostics.push(
                            &id.with_member(name),
                            "event stream variants must target structures",
                        );
                    }
                }
            }
        }
        if shape.traits.contains_key(&trait_id("requiresLength"))
            && (!streaming || shape.kind != Kind::Blob)
        {
            self.diagnostics
                .push(id, "@requiresLength must target a streaming blob");
        }
        let payloads: Vec<_> = shape
            .members
            .iter()
            .filter(|(_, m)| m.traits.contains_key(&trait_id("eventPayload")))
            .collect();
        if payloads.len() > 1 {
            self.diagnostics
                .push(id, "only one member may bind @eventPayload");
        }
        if !payloads.is_empty() {
            for (name, m) in &shape.members {
                if !m.traits.contains_key(&trait_id("eventPayload"))
                    && !m.traits.contains_key(&trait_id("eventHeader"))
                {
                    self.diagnostics.push(
                        &id.with_member(name),
                        "every other member beside @eventPayload must bind @eventHeader",
                    );
                }
            }
        }
    }

    pub(crate) fn event_binding(
        &mut self,
        id: &ShapeId,
        member: &ast::Member,
    ) -> Option<EventBinding> {
        let header = member.traits.contains_key(&trait_id("eventHeader"));
        let payload = member.traits.contains_key(&trait_id("eventPayload"));
        if !header && !payload {
            return None;
        }
        self.require(
            id,
            Feature::EventStreams,
            trait_id(if header {
                "eventHeader"
            } else {
                "eventPayload"
            }),
        );
        if header && payload {
            self.diagnostics
                .push(id, "@eventHeader and @eventPayload conflict");
        }
        let valid = self.model.get(&member.target).is_some_and(|target| {
            if header {
                matches!(
                    target.kind,
                    Kind::Boolean
                        | Kind::Byte
                        | Kind::Short
                        | Kind::Integer
                        | Kind::Long
                        | Kind::Blob
                        | Kind::String
                        | Kind::Enum
                        | Kind::Timestamp
                )
            } else {
                matches!(
                    target.kind,
                    Kind::Blob | Kind::String | Kind::Enum | Kind::Structure | Kind::Union
                )
            }
        });
        if !valid {
            self.diagnostics
                .push(id, "event binding targets an unsupported shape type");
        }
        Some(if header {
            EventBinding::Header
        } else {
            EventBinding::Payload
        })
    }

    pub(crate) fn check_stream_references(&mut self, operations: &[Operation]) {
        let inputs: BTreeSet<_> = operations
            .iter()
            .filter_map(|op| op.input.as_ref())
            .collect();
        let io: BTreeSet<_> = operations
            .iter()
            .flat_map(|op| op.input.iter().chain(op.output.iter()))
            .collect();
        // Only inspect reachable shapes; unrelated services may use other semantics.
        let reached: Vec<_> = self.reached_ids().collect();
        let mut containers = BTreeSet::new();
        for id in &reached {
            let shape = &self.model[id];
            let mut count = 0;
            for (name, member) in &shape.members {
                let Some(target) = self.model.get(&member.target) else {
                    continue;
                };
                if !target.traits.contains_key(&trait_id("streaming")) {
                    continue;
                }
                count += 1;
                containers.insert(id.clone());
                let member_id = id.with_member(name);
                if !io.contains(id) {
                    self.diagnostics.push(
                        &member_id,
                        "streaming shapes must be top-level operation input or output members",
                    );
                }
                if target.kind == Kind::Blob
                    && !member.traits.contains_key(&trait_id("required"))
                    && !member.traits.contains_key(&trait_id("default"))
                {
                    self.diagnostics.push(
                        &member_id,
                        "streaming blob members must be required or defaulted",
                    );
                }
                if target.traits.contains_key(&trait_id("requiresLength")) && !inputs.contains(id) {
                    self.diagnostics.push(
                        &member_id,
                        "@requiresLength can only be used in operation input",
                    );
                }
                if self.protocol_uses_http_bindings()
                    && !member.traits.contains_key(&trait_id("httpPayload"))
                {
                    self.diagnostics.push(
                        &member_id,
                        "streaming members must bind @httpPayload for this protocol",
                    );
                }
            }
            if count > 1 {
                self.diagnostics
                    .push(id, "only one member may target a streaming shape");
            }
        }
        for id in reached {
            let shape = &self.model[&id];
            for (name, member) in &shape.members {
                if containers.contains(&member.target) {
                    self.diagnostics.push(
                        &id.with_member(name),
                        "structures containing streams cannot be nested",
                    );
                }
            }
            for member in shape
                .member
                .iter()
                .chain(shape.key.iter())
                .chain(shape.value.iter())
            {
                if containers.contains(&member.target)
                    || self
                        .model
                        .get(&member.target)
                        .is_some_and(|s| s.traits.contains_key(&trait_id("streaming")))
                {
                    self.diagnostics
                        .push(&id, "streams cannot be nested in collections");
                }
            }
        }
    }
}
