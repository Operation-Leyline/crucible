//! Server conformance tests: the `smithy.test` vectors a service shares with
//! its clients, as a thin `#[test]` module.
//!
//! The output is a vector table, a `ParamShape` table describing how each
//! vector's Smithy `params` differ from the generated model's wire form, and
//! one call into `crucible_server_runtime_rust::conformance` per vector.
//! Nothing here decides pass or fail: that is the harness's job, written once.
//! `crucible-build` writes the file; the service includes it with
//! `crucible_server_runtime_rust::include_conformance!()`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crucible_smithy::ir::{
    HttpMessageExpectation, HttpProtocolTest, HttpTestCase, MalformedBodyAssertion,
    MalformedResponseBody, Operation, Segment, Service, Shape, SimpleKind, TestAppliesTo,
    TimestampFormat,
};
use crucible_smithy::{Diagnostics, Protocol, ShapeId};

use crate::names::{literal, snake};

/// Generated tests are not held to the lints of hand-written code.
const ALLOW: &str = "#[allow(clippy::all, non_snake_case)]";
use crate::{events, server_api};

/// A vector the service is known to fail, kept visible rather than dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    /// The vector's id, as `crucible conformance` exports it: the full id of an
    /// expanded `testParameters` case (`Id[name=value]`), or the template id
    /// to skip every case.
    pub vector: String,
    /// Why. A skipped test is `#[ignore = "reason"]`, so it shows in the run.
    pub reason: String,
}

/// A request an operation is known to accept, for the response vectors of an
/// operation that has no request vector and cannot be called empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seed {
    /// The operation's name.
    pub operation: String,
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ConformanceOptions {
    pub skip: Vec<Skip>,
    pub seeds: Vec<Seed>,
}

/// The conformance test module for `service`, or every reason there is none.
pub fn generate_conformance(
    service: &Service,
    options: &ConformanceOptions,
) -> Result<String, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    if service.protocol == Protocol::SmithyRpcV2Cbor {
        diagnostics.push(
            &service.id,
            "server conformance does not support smithy.protocols#rpcv2Cbor yet: \
             its documents are CBOR, and the harness translates params to JSON",
        );
        return Err(diagnostics);
    }
    let protocol = service.protocol.trait_id();
    let mut selected: Vec<&HttpProtocolTest> = Vec::new();
    let mut other_protocol = 0usize;
    let mut client_only = 0usize;
    for test in &service.protocol_tests {
        if test.applies_to == Some(TestAppliesTo::Client) {
            client_only += 1;
        } else if test.protocol != protocol {
            other_protocol += 1;
        } else {
            selected.push(test);
        }
    }
    if selected.is_empty() {
        diagnostics.push(
            &service.id,
            format!(
                "no smithy.test vector applies to this server: {} for another protocol than {protocol}, {client_only} client-only",
                other_protocol
            ),
        );
        return Err(diagnostics);
    }
    for skip in &options.skip {
        if !selected.iter().any(|test| skips(&skip.vector, test)) {
            diagnostics.push(
                &service.id,
                format!(
                    "conformance skip `{}` names no vector of this server",
                    skip.vector
                ),
            );
        }
    }

    for seed in &options.seeds {
        if !service
            .operations
            .iter()
            .any(|op| op.name() == seed.operation)
        {
            diagnostics.push(
                &service.id,
                format!("conformance seed names no operation `{}`", seed.operation),
            );
        }
    }
    let mut out = Generator::new(service, options);
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut tests = String::new();
    for test in &selected {
        let skip = options.skip.iter().find(|skip| skips(&skip.vector, test));
        match out.test(test, &selected) {
            Ok(body) => {
                let name = unique(&mut names, test);
                writeln!(tests, "\n#[test]\n{ALLOW}").unwrap();
                if let Some(skip) = skip {
                    writeln!(tests, "#[ignore = {}]", literal(&skip.reason)).unwrap();
                }
                writeln!(tests, "fn {name}() {{\n    {body}\n}}").unwrap();
            }
            Err(message) => match skip {
                Some(skip) => {
                    let name = unique(&mut names, test);
                    writeln!(tests, "\n#[test]\n{ALLOW}").unwrap();
                    writeln!(tests, "#[ignore = {}]", literal(&skip.reason)).unwrap();
                    writeln!(tests, "fn {name}() {{}}").unwrap();
                }
                None => diagnostics.push(
                    &test.operation,
                    format!("protocol test {}: {message}", test.id),
                ),
            },
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(out.finish(&tests, selected.len(), other_protocol, client_only))
}

fn skips(skip: &str, test: &HttpProtocolTest) -> bool {
    skip == test.id
        || test
            .id
            .split_once('[')
            .is_some_and(|(template, _)| skip == template)
}

/// A test function name: the operation, the kind of vector, the id; made
/// unique, since two ids can spell the same after sanitizing.
fn unique(seen: &mut BTreeMap<String, usize>, test: &HttpProtocolTest) -> String {
    let kind = match (&test.case, test.error) {
        (HttpTestCase::Request { .. }, _) => "request",
        (HttpTestCase::Response { .. }, false) => "response",
        (HttpTestCase::Response { .. }, true) => "error",
        (HttpTestCase::MalformedRequest { .. }, _) => "malformed",
    };
    let id: String = test
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    let base = format!(
        "{}_{kind}_{}",
        snake(test.operation.name()),
        snake(id.trim())
    );
    let count = seen.entry(base.clone()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base
    } else {
        format!("{base}_{count}")
    }
}

struct Generator<'a> {
    service: &'a Service,
    options: &'a ConformanceOptions,
    /// `ParamShape` statics by the shape they describe, in discovery order.
    shapes: BTreeMap<ShapeId, usize>,
    /// Why a cached shape cannot be described by params, so a later vector
    /// reaching it is diagnosed too, not only the first.
    shape_unsupported: BTreeMap<ShapeId, String>,
    bodies: Vec<String>,
    unsupported: Option<String>,
}

impl<'a> Generator<'a> {
    fn new(service: &'a Service, options: &'a ConformanceOptions) -> Self {
        Self {
            service,
            options,
            shapes: BTreeMap::new(),
            shape_unsupported: BTreeMap::new(),
            bodies: Vec::new(),
            unsupported: None,
        }
    }

    fn module(&self) -> String {
        format!("crate::{}", server_api::module(self.service))
    }

    /// The statement a test runs.
    fn test(
        &mut self,
        test: &HttpProtocolTest,
        siblings: &[&HttpProtocolTest],
    ) -> Result<String, String> {
        if test.vendor_params.is_some() || test.vendor_params_shape.is_some() {
            return Err(
                "vendorParams configure a specific implementation and cannot be applied here"
                    .into(),
            );
        }
        if test.auth_scheme.is_some() {
            return Err(
                "authScheme needs the service's authentication, which conformance does not configure"
                    .into(),
            );
        }
        let operation = self
            .service
            .operations
            .iter()
            .find(|operation| operation.id == test.operation)
            .ok_or("its operation is not in the service")?;
        let module = self.module();
        let marker = format!("{module}::{}", operation.name());
        match &test.case {
            HttpTestCase::Request {
                method,
                uri,
                host,
                query_params,
                message,
                ..
            } => {
                let shape = self.vector_shape(test.shape.as_ref())?;
                let request = http_request(
                    method,
                    uri,
                    host.as_deref(),
                    query_params,
                    &message.headers,
                    message.body.as_deref(),
                    message.body_media_type.as_deref(),
                );
                Ok(format!(
                    "::crucible_server_runtime_rust::conformance::request::<{marker}>(__conformance_build, &::crucible_server_runtime_rust::conformance::RequestVector {{ id: {}, params: {}, shape: {shape}, request: {request} }});",
                    literal(&test.id),
                    literal(&params(test)),
                ))
            }
            HttpTestCase::Response { code, message } => {
                let shape = self.vector_shape(test.shape.as_ref())?;
                let seed = self.seed(operation, siblings)?;
                let vector = format!(
                    "&::crucible_server_runtime_rust::conformance::ResponseVector {{ id: {}, params: {}, shape: {shape}, seed: {seed}, expect: {} }}",
                    literal(&test.id),
                    literal(&params(test)),
                    expected_response(*code, message),
                );
                if test.error {
                    let error = test
                        .shape
                        .as_ref()
                        .ok_or("an error vector names no error")?;
                    Ok(format!(
                        "::crucible_server_runtime_rust::conformance::error::<{marker}, {module}::{}>(__conformance_build, {vector});",
                        self.service.name_of(error)
                    ))
                } else {
                    Ok(format!(
                        "::crucible_server_runtime_rust::conformance::response::<{marker}>(__conformance_build, {vector});"
                    ))
                }
            }
            HttpTestCase::MalformedRequest { request, response } => {
                let http = http_request(
                    &request.method,
                    &request.uri,
                    request.host.as_deref(),
                    &request.query_params,
                    &request.headers,
                    request.body.as_deref(),
                    request.body_media_type.as_deref(),
                );
                Ok(format!(
                    "::crucible_server_runtime_rust::conformance::malformed(__conformance_build, &::crucible_server_runtime_rust::conformance::MalformedVector {{ id: {}, request: {http}, refusal: {} }});",
                    literal(&test.id),
                    refusal(response.code, &response.headers, response.body.as_ref()),
                ))
            }
        }
    }

    /// A request the operation accepts, for a response vector to be answered
    /// through: the one the model's owner named, else the operation's own when
    /// it needs no input, else the first request vector it has.
    fn seed(
        &self,
        operation: &Operation,
        siblings: &[&HttpProtocolTest],
    ) -> Result<String, String> {
        if let Some(seed) = self
            .options
            .seeds
            .iter()
            .find(|seed| seed.operation == operation.name())
        {
            return Ok(http_request(
                &seed.method,
                &seed.uri,
                None,
                &[],
                &seed.headers.iter().cloned().collect(),
                seed.body.as_deref(),
                None,
            ));
        }
        if self.service.protocol.uses_http_bindings() && self.needs_no_input(operation) {
            let mut uri = String::new();
            for segment in &operation.uri.segments {
                if let Segment::Literal(text) = segment {
                    uri.push('/');
                    uri.push_str(text);
                }
            }
            if uri.is_empty() {
                uri.push('/');
            }
            let query: Vec<String> = operation
                .uri
                .query
                .iter()
                .map(|(name, value)| match value {
                    Some(value) => format!("{name}={value}"),
                    None => name.clone(),
                })
                .collect();
            return Ok(http_request(
                &operation.method,
                &uri,
                None,
                &query,
                &BTreeMap::new(),
                None,
                None,
            ));
        }
        siblings
            .iter()
            .find_map(|candidate| match &candidate.case {
                HttpTestCase::Request {
                    method,
                    uri,
                    host,
                    query_params,
                    message,
                    ..
                } if candidate.operation == operation.id => Some(http_request(
                    method,
                    uri,
                    host.as_deref(),
                    query_params,
                    &message.headers,
                    message.body.as_deref(),
                    message.body_media_type.as_deref(),
                )),
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "{} needs a request to answer through, and has neither an input-free route nor a request vector",
                    operation.name()
                )
            })
    }

    /// Whether an empty request is a valid input.
    fn needs_no_input(&self, operation: &Operation) -> bool {
        if operation.uri.labels().next().is_some() {
            return false;
        }
        match operation
            .input
            .as_ref()
            .and_then(|id| self.service.shape(id))
        {
            Some(Shape::Structure(input)) => input
                .members
                .iter()
                .all(|member| !member.required || member.default.is_some()),
            _ => true,
        }
    }

    /// The `&SHAPE_n` expression describing a vector's shape. A missing shape
    /// is `Unit`: no members.
    fn vector_shape(&mut self, shape: Option<&ShapeId>) -> Result<String, String> {
        self.unsupported = None;
        let expression = match shape {
            Some(id) => self.shape(id),
            None => "&SHAPE_UNIT".to_owned(),
        };
        match self.unsupported.take() {
            Some(reason) => Err(reason),
            None => Ok(expression),
        }
    }

    fn shape(&mut self, target: &ShapeId) -> String {
        if events::is_event_stream(self.service, target) {
            self.unsupported = Some(format!(
                "reaches event stream {target}, which params cannot describe"
            ));
            return "&SHAPE_PLAIN".to_owned();
        }
        match self.service.shape(target) {
            Some(Shape::Simple(simple)) => {
                if simple.streaming {
                    self.unsupported = Some(format!(
                        "reaches streaming blob {target}: a stream cannot be compared with params"
                    ));
                }
                match simple.kind {
                    SimpleKind::Blob => "&SHAPE_BLOB".to_owned(),
                    SimpleKind::BigInteger | SimpleKind::BigDecimal
                        if self.service.protocol_settings.arbitrary_precision_strings =>
                    {
                        "&SHAPE_NUMBER_TEXT".to_owned()
                    }
                    // Timestamps depend on the member holding them; `member` handles those.
                    _ => "&SHAPE_PLAIN".to_owned(),
                }
            }
            Some(Shape::Unit { .. }) | None => "&SHAPE_UNIT".to_owned(),
            Some(Shape::Enum(_) | Shape::IntEnum(_)) => "&SHAPE_PLAIN".to_owned(),
            Some(
                shape @ (Shape::List(_) | Shape::Map(_) | Shape::Structure(_) | Shape::Union(_)),
            ) => {
                if let Some(index) = self.shapes.get(target) {
                    if let Some(reason) = self.shape_unsupported.get(target) {
                        self.unsupported.get_or_insert_with(|| reason.clone());
                    }
                    return format!("&SHAPE_{index}");
                }
                // Registered before its members are visited, so recursive
                // models refer back to it.
                let index = self.bodies.len();
                self.shapes.insert(target.clone(), index);
                self.bodies.push(String::new());
                let outer = self.unsupported.take();
                let body = match shape {
                    Shape::List(list) => format!("ParamShape::List({})", self.member(&list.member)),
                    Shape::Map(map) => format!("ParamShape::Map({})", self.member(&map.value)),
                    Shape::Structure(structure) | Shape::Union(structure) => {
                        let members: Vec<String> = structure
                            .members
                            .iter()
                            .map(|member| {
                                format!(
                                    "ParamMember {{ name: {}, wire: {}, shape: {} }}",
                                    literal(&member.name),
                                    literal(&member.wire_name),
                                    self.member(member)
                                )
                            })
                            .collect();
                        let kind = if matches!(shape, Shape::Union(_)) {
                            "Union"
                        } else {
                            "Structure"
                        };
                        format!("ParamShape::{kind}(&[{}])", members.join(", "))
                    }
                    _ => unreachable!("matched above"),
                };
                if let Some(reason) = self.unsupported.clone() {
                    self.shape_unsupported.insert(target.clone(), reason);
                }
                if outer.is_some() {
                    self.unsupported = outer;
                }
                self.bodies[index] = body;
                format!("&SHAPE_{index}")
            }
        }
    }

    /// A member's shape: its target's, except that a timestamp is written in
    /// the format the member's document uses.
    fn member(&mut self, member: &crucible_smithy::ir::Member) -> String {
        if let Some(Shape::Simple(simple)) = self.service.shape(&member.target)
            && simple.kind == SimpleKind::Timestamp
        {
            return match self.service.document_timestamp_format(member) {
                TimestampFormat::DateTime => "&SHAPE_DATE_TIME",
                TimestampFormat::EpochSeconds => "&SHAPE_EPOCH_SECONDS",
                TimestampFormat::HttpDate => "&SHAPE_HTTP_DATE",
            }
            .to_owned();
        }
        self.shape(&member.target)
    }

    fn finish(
        &self,
        tests: &str,
        vectors: usize,
        other_protocol: usize,
        client_only: usize,
    ) -> String {
        let service = self.service;
        let module = self.module();
        let mut out = String::new();
        writeln!(
            out,
            "// Generated by crucible from {} {}. Do not edit: change the model.",
            service.id, service.version
        )
        .unwrap();
        writeln!(
            out,
            "// {vectors} smithy.test vectors; not generated: {other_protocol} for another protocol, {client_only} client-only.\n"
        )
        .unwrap();
        out.push_str("#[allow(unused_imports)]\nuse ::crucible_server_runtime_rust::conformance::{ParamMember, ParamShape};\n");
        out.push_str("#[allow(clippy::all)]\nfn __conformance_build(stub: ::std::sync::Arc<::crucible_server_runtime_rust::conformance::Stub>) -> ::crucible_server_runtime_rust::Crucible {\n");
        out.push_str(&format!(
            "    let operations = ::crucible_server_runtime_rust::Operations::builder(&{module}::SERVICE)\n"
        ));
        for operation in &service.operations {
            let marker = format!("{module}::{}", operation.name());
            writeln!(
                out,
                "        .add::<{marker}, _>(stub.clone(), ::crucible_server_runtime_rust::conformance::Stub::call::<{marker}>)"
            )
            .unwrap();
        }
        out.push_str("        .build()\n        .expect(\"every operation is registered\");\n");
        out.push_str("    __conformance_configure(::crucible_server_runtime_rust::Crucible::builder(operations).defaults())\n        .build()\n        .expect(\"the service builds\")\n}\n\n");
        // The shared shapes are all emitted; a model uses only some of them.
        for (name, shape) in [
            ("SHAPE_PLAIN", "Plain".to_owned()),
            ("SHAPE_BLOB", "Blob".to_owned()),
            ("SHAPE_NUMBER_TEXT", "NumberText".to_owned()),
            ("SHAPE_UNIT", "Structure(&[])".to_owned()),
            ("SHAPE_DATE_TIME", timestamp("DateTime")),
            ("SHAPE_EPOCH_SECONDS", timestamp("EpochSeconds")),
            ("SHAPE_HTTP_DATE", timestamp("HttpDate")),
        ] {
            writeln!(
                out,
                "#[allow(dead_code)]
static {name}: ParamShape = ParamShape::{shape};"
            )
            .unwrap();
        }
        for (index, body) in self.bodies.iter().enumerate() {
            writeln!(out, "static SHAPE_{index}: ParamShape = {body};").unwrap();
        }
        out.push_str(tests);
        out
    }
}

fn timestamp(format: &str) -> String {
    format!("Timestamp(::crucible_server_runtime_rust::operation::TimestampFormat::{format})")
}

fn params(test: &HttpProtocolTest) -> String {
    serde_json::to_string(&test.params).expect("params serialize")
}

fn pairs(map: &BTreeMap<String, String>) -> String {
    let items: Vec<String> = map
        .iter()
        .map(|(name, value)| format!("({}, {})", literal(name), literal(value)))
        .collect();
    format!("&[{}]", items.join(", "))
}

fn strings(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| literal(item)).collect();
    format!("&[{}]", items.join(", "))
}

fn option(text: Option<&str>) -> String {
    match text {
        Some(text) => format!("Some({})", literal(text)),
        None => "None".to_owned(),
    }
}

fn http_request(
    method: &str,
    uri: &str,
    host: Option<&str>,
    query: &[String],
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
    body_media_type: Option<&str>,
) -> String {
    format!(
        "::crucible_server_runtime_rust::conformance::HttpRequest {{ method: {}, uri: {}, host: {}, query: {}, headers: {}, body: {}, body_media_type: {} }}",
        literal(method),
        literal(uri),
        option(host),
        strings(query),
        pairs(headers),
        option(body),
        option(body_media_type),
    )
}

fn expected_response(code: u16, message: &HttpMessageExpectation) -> String {
    format!(
        "::crucible_server_runtime_rust::conformance::ExpectedResponse {{ code: {code}, headers: {}, forbid_headers: {}, require_headers: {}, body: {}, body_media_type: {} }}",
        pairs(&message.headers),
        strings(&message.forbid_headers),
        strings(&message.require_headers),
        option(message.body.as_deref()),
        option(message.body_media_type.as_deref()),
    )
}

fn refusal(
    code: u16,
    headers: &BTreeMap<String, String>,
    body: Option<&MalformedResponseBody>,
) -> String {
    let body = match body {
        Some(body) => {
            let assertion = match &body.assertion {
                MalformedBodyAssertion::Contents(text) => format!(
                    "::crucible_server_runtime_rust::conformance::BodyAssertion::Contents({})",
                    literal(text)
                ),
                MalformedBodyAssertion::MessageRegex(pattern) => format!(
                    "::crucible_server_runtime_rust::conformance::BodyAssertion::MessageRegex({})",
                    literal(pattern)
                ),
            };
            format!("Some(({}, {assertion}))", literal(&body.media_type))
        }
        None => "None".to_owned(),
    };
    format!(
        "::crucible_server_runtime_rust::conformance::Refusal {{ code: {code}, headers: {}, body: {body} }}",
        pairs(headers)
    )
}
