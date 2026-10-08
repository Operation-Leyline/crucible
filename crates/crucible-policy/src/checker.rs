use crate::direction::Direction;
use crate::{Report, Severity};
use crucible_smithy::ir::{Bindings, Constraints, Member, Service, Shape, Structure};
use crucible_smithy::{Feature, Protocol, ShapeId};
use std::cmp::Ordering;
use std::collections::BTreeSet;

pub(crate) struct Checker<'a> {
    pub(crate) old: &'a Service,
    pub(crate) new: &'a Service,
    pub(crate) severity: Severity,
    pub(crate) report: Report,
    pub(crate) seen: BTreeSet<(ShapeId, ShapeId, Direction, bool)>,
}

impl Checker<'_> {
    fn push(&mut self, rule: &str, id: &ShapeId, message: impl Into<String>) {
        self.report.push(rule, id, self.severity, message);
    }

    pub(crate) fn wire(&mut self) {
        if self.old.id != self.new.id {
            self.push(
                "compatibility.service",
                &self.old.id.clone(),
                "service identity changed",
            );
        }
        if self.old.protocol != self.new.protocol
            || self.old.protocol_settings != self.new.protocol_settings
        {
            self.push(
                "compatibility.protocol",
                &self.old.id.clone(),
                "wire protocol or its settings changed",
            );
        }
        for service in [self.old, self.new] {
            if !matches!(
                service.protocol,
                Protocol::CrucibleJson | Protocol::AwsRestJson1
            ) {
                self.push(
                    "compatibility.unchecked",
                    &service.id,
                    "wire compatibility currently supports Crucible JSON and restJson1 only",
                );
            }
            for usage in &service.requirements {
                if !matches!(
                    usage.feature,
                    Feature::JsonName
                        | Feature::GreedyLabels
                        | Feature::QueryParams
                        | Feature::PrefixHeaders
                        | Feature::ResponseCode
                        | Feature::Sparse
                        | Feature::Sets
                        | Feature::IntEnum
                        | Feature::BigInteger
                        | Feature::BigDecimal
                        | Feature::NonBlobPayload
                        | Feature::ErrorPayload
                        | Feature::MediaTypeHeaders
                        | Feature::OptionalAuth
                        | Feature::AwsRestJson1
                ) {
                    self.push(
                        "compatibility.unchecked",
                        &usage.shape,
                        format!("wire compatibility does not yet assess {}", usage.feature),
                    );
                }
            }
        }
        // Metadata outside the supported wire subset must not silently pass.
        if self.old.endpoint_rules.is_some()
            || self.new.endpoint_rules.is_some()
            || self.old.endpoint_discovery.is_some()
            || self.new.endpoint_discovery.is_some()
        {
            self.push(
                "compatibility.unchecked",
                &self.old.id.clone(),
                "endpoint rules/discovery require a separate compatibility assessment",
            );
        }
        for old_op in &self.old.operations {
            let Some(new_op) = self.new.operations.iter().find(|op| op.id == old_op.id) else {
                self.push(
                    "compatibility.operation",
                    &old_op.id,
                    "operation was removed or renamed",
                );
                continue;
            };
            if old_op.method != new_op.method
                || old_op.uri != new_op.uri
                || old_op.status != new_op.status
            {
                self.push(
                    "compatibility.http",
                    &old_op.id,
                    "HTTP method, URI pattern, or success status changed",
                );
            }
            if old_op.auth != new_op.auth
                || old_op.optional_auth != new_op.optional_auth
                || self.old.auth_schemes != self.new.auth_schemes
            {
                self.push(
                    "compatibility.auth",
                    &old_op.id,
                    "authentication contract changed; assess existing credentials and callers",
                );
            }
            if old_op.paginated != new_op.paginated && old_op.paginated.is_some() {
                self.push(
                    "compatibility.pagination",
                    &old_op.id,
                    "existing pagination contract changed",
                );
            }
            if old_op.idempotent && !new_op.idempotent
                || old_op.idempotency_token != new_op.idempotency_token
            {
                self.push(
                    "compatibility.operation",
                    &old_op.id,
                    "idempotency contract changed",
                );
            }
            if old_op.readonly && !new_op.readonly {
                self.push(
                    "compatibility.operation",
                    &old_op.id,
                    "readonly contract changed; existing clients may retry requests that now have side effects",
                );
            }
            self.root(
                &old_op.id,
                old_op.input.as_ref(),
                new_op.input.as_ref(),
                Direction::Input,
                &old_op.request,
                &new_op.request,
            );
            self.root(
                &old_op.id,
                old_op.output.as_ref(),
                new_op.output.as_ref(),
                Direction::Output,
                &old_op.response,
                &new_op.response,
            );
            let old_codes: BTreeSet<_> = old_op
                .errors
                .iter()
                .filter_map(|id| error_code(self.old, id))
                .collect();
            for new_error in &new_op.errors {
                let Some(code) = error_code(self.new, new_error) else {
                    continue;
                };
                if !old_codes.contains(&code) {
                    self.push(
                        "compatibility.error",
                        &old_op.id,
                        format!("new modelled error {code:?} can reach old clients"),
                    );
                }
            }
            for old_error in &old_op.errors {
                let old_info = error_info(self.old, old_error);
                let new_error = new_op
                    .errors
                    .iter()
                    .find(|id| error_code(self.new, id) == error_code(self.old, old_error));
                if let Some(new_error) = new_error {
                    let new_info = error_info(self.new, new_error);
                    if let (Some(a), Some(b)) = (old_info, new_info) {
                        if a.status != b.status
                            || a.fault != b.fault
                            || a.retryable != b.retryable
                            || a.payload != b.payload
                            || a.headers != b.headers
                            || a.prefix_headers != b.prefix_headers
                            || a.response_code != b.response_code
                            || a.timestamp_formats != b.timestamp_formats
                            || a.header_encodings != b.header_encodings
                        {
                            self.push(
                                "compatibility.error",
                                old_error,
                                "error status, fault, retryability, or wire bindings changed",
                            );
                        }
                    }
                    self.shape(old_error, new_error, Direction::Output, None);
                }
            }
        }
    }

    fn root(
        &mut self,
        operation: &ShapeId,
        old: Option<&ShapeId>,
        new: Option<&ShapeId>,
        direction: Direction,
        old_bindings: &Bindings,
        new_bindings: &Bindings,
    ) {
        if old_bindings.payload != new_bindings.payload
            || old_bindings.response_code != new_bindings.response_code
            || old_bindings.header_encodings != new_bindings.header_encodings
        {
            self.push(
                "compatibility.bindings",
                operation,
                "payload, dynamic status, or header encoding changed",
            );
        }
        match (old, new) {
            (Some(old), Some(new)) => {
                self.shape(old, new, direction, Some((old_bindings, new_bindings)))
            }
            (None, Some(new)) => {
                if direction == Direction::Input {
                    if let Some(Shape::Structure(shape)) = self.new.shape(new) {
                        for member in &shape.members {
                            if !member.optional_on_server() && member.default.is_none() {
                                self.push(
                                    "compatibility.required",
                                    &new.with_member(&member.name),
                                    "new input requires a member old callers do not send",
                                );
                            }
                        }
                    } else {
                        self.push(
                            "compatibility.type",
                            operation,
                            "operation now requires an input payload",
                        );
                    }
                }
            }
            (Some(_), None) => self.push(
                "compatibility.type",
                operation,
                "operation input or output was removed",
            ),
            (None, None) => {}
        }
    }

    fn shape(
        &mut self,
        old_id: &ShapeId,
        new_id: &ShapeId,
        direction: Direction,
        bindings: Option<(&Bindings, &Bindings)>,
    ) {
        if !self.seen.insert((
            old_id.clone(),
            new_id.clone(),
            direction,
            bindings.is_some(),
        )) {
            return;
        }
        let (Some(old), Some(new)) = (self.old.shape(old_id), self.new.shape(new_id)) else {
            self.push("compatibility.type", old_id, "referenced type is missing");
            return;
        };
        match (old, new) {
            (Shape::Unit { .. }, Shape::Unit { .. }) => {}
            (Shape::Simple(a), Shape::Simple(b)) => {
                if a.kind != b.kind
                    || a.streaming != b.streaming
                    || a.requires_length != b.requires_length
                {
                    self.push(
                        "compatibility.type",
                        old_id,
                        "scalar type or streaming contract changed",
                    );
                }
                // Incoming members already carry effective constraints and timestamp
                // formats. Raw target traits may be overridden at every wire use.
            }
            (Shape::Enum(a), Shape::Enum(b)) => {
                let a = a.values.iter().map(|v| v.value.as_str()).collect();
                let b = b.values.iter().map(|v| v.value.as_str()).collect();
                self.values(old_id, a, b, direction);
            }
            (Shape::IntEnum(a), Shape::IntEnum(b)) => {
                let a = a.values.iter().map(|v| v.value).collect::<BTreeSet<_>>();
                let b = b.values.iter().map(|v| v.value).collect::<BTreeSet<_>>();
                if direction == Direction::Input && !a.is_subset(&b)
                    || direction == Direction::Output && !b.is_subset(&a)
                {
                    self.push(
                        "compatibility.enum",
                        old_id,
                        "accepted input values were removed or possible output values were added",
                    );
                }
            }
            (Shape::Structure(a), Shape::Structure(b)) => {
                let strict = ShapeId::new("crucible.protocols", "strict");
                if direction == Direction::Input
                    && !a.traits.contains_key(&strict)
                    && b.traits.contains_key(&strict)
                {
                    self.push(
                        "compatibility.constraints",
                        old_id,
                        "input now rejects unknown members",
                    );
                }
                self.members(old_id, a, b, direction, bindings, false)
            }
            (Shape::Union(a), Shape::Union(b)) => {
                self.members(old_id, a, b, direction, bindings, true)
            }
            (Shape::List(a), Shape::List(b)) => {
                if a.is_set != b.is_set {
                    self.push(
                        "compatibility.type",
                        old_id,
                        "collection set/list semantics changed",
                    );
                }
                if direction == Direction::Input && a.sparse && !b.sparse
                    || direction == Direction::Output && !a.sparse && b.sparse
                {
                    self.push(
                        "compatibility.nullability",
                        old_id,
                        "collection nullability changed incompatibly",
                    );
                }
                self.member(
                    &old_id.with_member("member"),
                    &a.member,
                    &b.member,
                    direction,
                    None,
                );
            }
            (Shape::Map(a), Shape::Map(b)) => {
                if direction == Direction::Input && a.sparse && !b.sparse
                    || direction == Direction::Output && !a.sparse && b.sparse
                {
                    self.push(
                        "compatibility.nullability",
                        old_id,
                        "collection nullability changed incompatibly",
                    );
                }
                self.member(&old_id.with_member("key"), &a.key, &b.key, direction, None);
                self.member(
                    &old_id.with_member("value"),
                    &a.value,
                    &b.value,
                    direction,
                    None,
                );
            }
            _ => self.push("compatibility.type", old_id, "shape kind changed"),
        }
    }

    fn values<'a>(
        &mut self,
        id: &ShapeId,
        old: BTreeSet<&'a str>,
        new: BTreeSet<&'a str>,
        direction: Direction,
    ) {
        if direction == Direction::Input && !old.is_subset(&new)
            || direction == Direction::Output && !new.is_subset(&old)
        {
            self.push(
                "compatibility.enum",
                id,
                "accepted input values were removed or possible output values were added",
            );
        }
    }

    fn members(
        &mut self,
        id: &ShapeId,
        old: &Structure,
        new: &Structure,
        direction: Direction,
        bindings: Option<(&Bindings, &Bindings)>,
        union: bool,
    ) {
        for old_member in &old.members {
            let old_location = location(old_member, bindings.map(|b| b.0));
            let new_member = new
                .members
                .iter()
                .find(|member| location(member, bindings.map(|b| b.1)) == old_location);
            match new_member {
                Some(new_member) => self.member(
                    &id.with_member(&old_member.name),
                    old_member,
                    new_member,
                    direction,
                    bindings,
                ),
                None => {
                    // Input readers may reject unknown fields; removal cannot promise compatibility.
                    if direction == Direction::Input || !union {
                        self.push(
                            "compatibility.member",
                            &id.with_member(&old_member.name),
                            format!("member at {old_location:?} was removed or rebound"),
                        );
                    }
                }
            }
        }
        for new_member in &new.members {
            let new_location = location(new_member, bindings.map(|b| b.1));
            if old
                .members
                .iter()
                .any(|member| location(member, bindings.map(|b| b.0)) == new_location)
            {
                continue;
            }
            if union && direction == Direction::Output {
                self.push(
                    "compatibility.enum",
                    &id.with_member(&new_member.name),
                    "new output union variant can reach old clients",
                );
            } else if !union
                && direction == Direction::Input
                && !new_member.optional_on_server()
                && new_member.default.is_none()
            {
                self.push(
                    "compatibility.required",
                    &id.with_member(&new_member.name),
                    "new required input has no default for old callers",
                );
            }
        }
    }

    fn member(
        &mut self,
        id: &ShapeId,
        old: &Member,
        new: &Member,
        direction: Direction,
        bindings: Option<(&Bindings, &Bindings)>,
    ) {
        let old_optional = if direction == Direction::Input {
            old.optional_on_server()
        } else {
            old.optional_on_client()
        };
        let new_optional = if direction == Direction::Input {
            new.optional_on_server()
        } else {
            new.optional_on_client()
        };
        if direction == Direction::Input && old_optional && !new_optional && new.default.is_none()
            || direction == Direction::Output && !old_optional && new_optional
        {
            self.push(
                "compatibility.nullability",
                id,
                "input became mandatory or output lost its non-null guarantee",
            );
        }
        if old.wire_default != new.wire_default || old.default != new.default {
            self.push(
                "compatibility.default",
                id,
                "default changed; omission can now have different semantics",
            );
        }
        let old_timestamp = bindings
            .and_then(|b| b.0.timestamp_formats.get(&old.name))
            .copied()
            .unwrap_or_else(|| self.old.timestamp_format(old));
        let new_timestamp = bindings
            .and_then(|b| b.1.timestamp_formats.get(&new.name))
            .copied()
            .unwrap_or_else(|| self.new.timestamp_format(new));
        if old_timestamp != new_timestamp {
            self.push(
                "compatibility.timestamp",
                id,
                "effective timestamp wire format changed",
            );
        }
        self.constraints(id, &old.constraints, &new.constraints, direction);
        self.shape(&old.target, &new.target, direction, None);
    }

    fn constraints(
        &mut self,
        id: &ShapeId,
        old: &Constraints,
        new: &Constraints,
        direction: Direction,
    ) {
        let (accepted, produced) = if direction == Direction::Input {
            (new, old)
        } else {
            (old, new)
        };
        let length_break = match (&accepted.length, &produced.length) {
            (None, _) => false,
            (Some(a), Some(b)) => {
                a.min.is_some_and(|v| b.min.is_none_or(|w| w < v))
                    || a.max.is_some_and(|v| b.max.is_none_or(|w| w > v))
            }
            (Some(a), None) => a.min.is_some() || a.max.is_some(),
        };
        let range_break = match (&accepted.range, &produced.range) {
            (None, _) => false,
            (Some(a), Some(b)) => {
                a.min.as_ref().is_some_and(|v| {
                    b.min
                        .as_ref()
                        .is_none_or(|w| compare_numbers(w, v) == Ordering::Less)
                }) || a.max.as_ref().is_some_and(|v| {
                    b.max
                        .as_ref()
                        .is_none_or(|w| compare_numbers(w, v) == Ordering::Greater)
                })
            }
            (Some(a), None) => a.min.is_some() || a.max.is_some(),
        };
        let pattern_break = old.pattern != new.pattern && accepted.pattern.is_some();
        let uniqueness_break = accepted.unique_items && !produced.unique_items;
        if length_break || range_break || pattern_break || uniqueness_break {
            self.push("compatibility.constraints", id, "constraints changed outside the previous contract (changed patterns require review)");
        }
    }

    pub(crate) fn sdk(&mut self) {
        if self.old.id != self.new.id || self.old.title != self.new.title {
            self.push(
                "compatibility.sdk",
                &self.old.id.clone(),
                "service identity or public client title changed",
            );
        }
        for old_op in &self.old.operations {
            let Some(new_op) = self.new.operations.iter().find(|op| op.id == old_op.id) else {
                self.push(
                    "compatibility.sdk",
                    &old_op.id,
                    "public operation symbol was removed",
                );
                continue;
            };
            if old_op.input != new_op.input
                || old_op.output != new_op.output
                || old_op.errors != new_op.errors
            {
                self.push(
                    "compatibility.sdk",
                    &old_op.id,
                    "operation's public input, output, or error types changed",
                );
            }
        }
        for (id, old) in &self.old.shapes {
            if id.is_prelude() {
                continue;
            }
            let Some(new) = self.new.shape(id) else {
                self.push(
                    "compatibility.sdk",
                    id,
                    "public type symbol was removed or renamed",
                );
                continue;
            };
            if std::mem::discriminant(old) != std::mem::discriminant(new) {
                self.push("compatibility.sdk", id, "public type kind changed");
            }
            if self.old.name_of(id) != self.new.name_of(id) {
                self.push(
                    "compatibility.sdk",
                    id,
                    "service-local public type name changed",
                );
            }
            match (old, new) {
                (Shape::Structure(a), Shape::Structure(b)) | (Shape::Union(a), Shape::Union(b)) => {
                    for member in &a.members {
                        match b.member(&member.name) {
                            Some(new)
                                if member.target == new.target
                                    && member.optional_on_client() == new.optional_on_client() => {}
                            _ => self.push(
                                "compatibility.sdk",
                                &id.with_member(&member.name),
                                "public member was removed or its type/nullability changed",
                            ),
                        }
                    }
                    for member in &b.members {
                        if a.member(&member.name).is_none() {
                            self.push(
                                "compatibility.sdk",
                                &id.with_member(&member.name),
                                "new public member can change construction or exhaustive matching APIs",
                            );
                        }
                    }
                }
                (Shape::Enum(a), Shape::Enum(b)) => {
                    for value in &a.values {
                        if !b
                            .values
                            .iter()
                            .any(|new| new.name == value.name && new.value == value.value)
                        {
                            self.push(
                                "compatibility.sdk",
                                &id.with_member(&value.name),
                                "public enum symbol was removed or its value changed",
                            );
                        }
                    }
                    if b.values
                        .iter()
                        .any(|value| !a.values.iter().any(|old| old.name == value.name))
                    {
                        self.push(
                            "compatibility.sdk",
                            id,
                            "new public enum variants can break exhaustive matching",
                        );
                    }
                }
                (Shape::IntEnum(a), Shape::IntEnum(b)) => {
                    for value in &a.values {
                        if !b
                            .values
                            .iter()
                            .any(|new| new.name == value.name && new.value == value.value)
                        {
                            self.push(
                                "compatibility.sdk",
                                &id.with_member(&value.name),
                                "public enum symbol was removed or its value changed",
                            );
                        }
                    }
                    if b.values
                        .iter()
                        .any(|value| !a.values.iter().any(|old| old.name == value.name))
                    {
                        self.push(
                            "compatibility.sdk",
                            id,
                            "new public enum variants can break exhaustive matching",
                        );
                    }
                }
                (Shape::Simple(a), Shape::Simple(b)) if a.kind != b.kind => {
                    self.push("compatibility.sdk", id, "public scalar type changed")
                }
                (Shape::List(a), Shape::List(b))
                    if a.member.target != b.member.target
                        || a.sparse != b.sparse
                        || a.is_set != b.is_set =>
                {
                    self.push("compatibility.sdk", id, "public collection type changed")
                }
                (Shape::Map(a), Shape::Map(b))
                    if a.key.target != b.key.target
                        || a.value.target != b.value.target
                        || a.sparse != b.sparse =>
                {
                    self.push("compatibility.sdk", id, "public map type changed")
                }
                _ => {}
            }
        }
    }
}

fn compare_numbers(a: &serde_json::Number, b: &serde_json::Number) -> Ordering {
    let integer = |number: &serde_json::Number| {
        number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from))
    };
    let float_integer = |value: f64, integer: i128| {
        // Casting the float to an integer preserves its represented value for
        // comparison with 64-bit integer bounds, without rounding the integer.
        let order = (value as i128).cmp(&integer);
        if order == Ordering::Equal {
            value.fract().partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        } else {
            order
        }
    };
    match (integer(a), integer(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(a), None) => float_integer(b.as_f64().expect("JSON numbers are finite"), a).reverse(),
        (None, Some(b)) => float_integer(a.as_f64().expect("JSON numbers are finite"), b),
        (None, None) => a
            .as_f64()
            .partial_cmp(&b.as_f64())
            .expect("JSON numbers are finite"),
    }
}

fn error_info<'a>(
    service: &'a Service,
    id: &ShapeId,
) -> Option<&'a crucible_smithy::ir::ErrorInfo> {
    match service.shape(id) {
        Some(Shape::Structure(shape)) => shape.error.as_ref(),
        _ => None,
    }
}

fn error_code<'a>(service: &'a Service, id: &ShapeId) -> Option<&'a str> {
    error_info(service, id).map(|info| info.wire_code.as_str())
}

fn location(member: &Member, bindings: Option<&Bindings>) -> String {
    if let Some(bindings) = bindings {
        if bindings.labels.contains(&member.name) {
            return format!("label:{}", member.name);
        }
        if let Some(binding) = bindings.query.iter().find(|b| b.member == member.name) {
            return format!("query:{}", binding.name);
        }
        if let Some(binding) = bindings.headers.iter().find(|b| b.member == member.name) {
            return format!("header:{}", binding.name.to_ascii_lowercase());
        }
        if let Some(binding) = bindings
            .prefix_headers
            .iter()
            .find(|b| b.member == member.name)
        {
            return format!("prefix-header:{}", binding.name.to_ascii_lowercase());
        }
        if bindings.query_params.as_deref() == Some(&member.name) {
            return "query-params".into();
        }
        if bindings
            .payload
            .as_ref()
            .is_some_and(|payload| payload.member == member.name)
        {
            return "payload".into();
        }
        if bindings.response_code.as_deref() == Some(&member.name) {
            return "response-code".into();
        }
    }
    format!("body:{}", member.wire_name)
}
