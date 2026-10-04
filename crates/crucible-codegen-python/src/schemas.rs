use crate::names::field;
use crucible_smithy::ir::{Bindings, Fault, Member, Service, Shape, SimpleKind, TimestampFormat};
use serde_json::{Value, json};

fn member(service: &Service, m: &Member) -> Value {
    json!({"target": m.target.to_string(), "attribute": field(&m.name),
        "required": m.required, "default": m.default,
        "format": format(service.timestamp_format(m))})
}

fn format(f: TimestampFormat) -> &'static str {
    match f {
        TimestampFormat::DateTime => "date-time",
        TimestampFormat::HttpDate => "http-date",
        TimestampFormat::EpochSeconds => "epoch-seconds",
    }
}

fn bindings(b: &Bindings) -> Value {
    json!({"labels": b.labels, "query": b.query.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(),
        "headers": b.headers.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(), "body": b.body,
        "payload": b.payload.as_ref().map(|p| json!([p.member, p.media_type]))})
}

pub fn schema(service: &Service) -> Value {
    let shapes: serde_json::Map<String, Value> = service.shapes.iter().map(|(id, shape)| {
        let mut value = match shape {
            Shape::Unit => json!({"kind":"unit"}),
            Shape::Simple(s) => json!({"kind": match s.kind {
                SimpleKind::String => "string", SimpleKind::Boolean => "boolean",
                SimpleKind::Byte => "byte", SimpleKind::Short => "short", SimpleKind::Integer => "integer", SimpleKind::Long => "long",
                SimpleKind::Float => "float", SimpleKind::Double => "double", SimpleKind::Timestamp => "timestamp", SimpleKind::Document => "document",
                SimpleKind::Blob => "blob",
            }, "format": format(s.timestamp_format.unwrap_or(TimestampFormat::DateTime))}),
            Shape::Enum(e) => json!({"kind":"enum", "values":e.values.iter().map(|v| &v.value).collect::<Vec<_>>()}),
            Shape::List(l) => json!({"kind":"list", "member":member(service,&l.member)}),
            Shape::Map(m) => json!({"kind":"map", "key":member(service,&m.key), "value":member(service,&m.value)}),
            Shape::Structure(s) | Shape::Union(s) => {
                let members: serde_json::Map<_,_> = s.members.iter().map(|m| (m.name.clone(),member(service,m))).collect();
                let mut v = json!({"kind": if matches!(shape,Shape::Union(_)) {"union"} else {"structure"}, "members":members});
                if let Some(e) = &s.error {
                    v["error"] = json!({"status":e.status,"fault":if e.fault == Fault::Client {"client"} else {"server"},
                        "retryable":e.retryable.is_some(),"throttling":e.retryable.is_some_and(|r|r.throttling),
                        "headers":e.headers.iter().map(|b|json!([b.member,b.name])).collect::<Vec<_>>()});
                }
                v
            }
        };
        value["name"] = json!(service.name_of(id));
        (id.to_string(),value)
    }).collect();
    let operations: serde_json::Map<String,Value> = service.operations.iter().map(|op| {
        let pagination = op.paginated.as_ref().map(|p|json!({"input":p.input_token,"output":p.output_token.split('.').collect::<Vec<_>>(),"items":p.items,"pageSize":p.page_size}));
        (op.name().into(),json!({"method":op.method,"uri":op.uri.to_string(),"input":op.input.as_ref().map(ToString::to_string),"output":op.output.as_ref().map(ToString::to_string),
            "request":bindings(&op.request),"response":bindings(&op.response),"errors":op.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "safe":op.readonly || op.idempotent || op.idempotency_token.is_some(),"token":op.idempotency_token,"pagination":pagination}))
    }).collect();
    json!({"service":service.id.to_string(),"version":service.version,"shapes":shapes,"operations":operations,
        "bearer":service.auth.iter().any(|id|id.to_string()=="smithy.api#httpBearerAuth")})
}
