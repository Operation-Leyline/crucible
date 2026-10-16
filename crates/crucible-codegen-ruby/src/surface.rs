//! The gem's public surface (`crucible.sdks/1`), named by the functions the
//! client and model classes are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::models::qualified_yard_type;
use crate::names::field;
use crate::{Options, client_name, operation_method, pages_method};

/// What the generated gem shows its callers: the gem and its require path,
/// the client class, and per operation its method, its `_pages`
/// enumerator, and the model classes it takes (keyword arguments to `new`)
/// and returns (attribute readers).
///
/// After `require`, the gem's constants live in its module, so every name
/// is qualified with it (`Weather::WeatherClient`,
/// `Weather::Runtime::Timestamp`). Types are the YARD types the generated
/// documentation states, qualified the same way.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let qualifier = format!("{}::", options.module_name);
    let client = format!("{qualifier}{}", client_name(service));
    let construct = if options.endpoint.is_some() {
        format!("{client}.new")
    } else {
        format!("{client}.new({qualifier}Runtime::ClientConfig.new(endpoint: endpoint))")
    };
    let mut package = Package::new("rubygems", &options.package_name, &options.package_name);
    package.imports = vec![format!("require '{}'", options.package_name)];
    package.qualifier.clone_from(&qualifier);
    let mut out = surface::Surface::new("ruby", package, ClientType::new(client, construct));
    out.endpoint.clone_from(&options.endpoint);
    for op in &service.operations {
        let method = operation_method(op);
        let mut entry = Operation::new(&method);
        entry.paginators = op.paginated.as_ref().map(|_| vec![pages_method(&method)]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, &qualifier, id, "keywords"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, &qualifier, id, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class: a keyword argument and an attribute reader per member.
fn structure(service: &Service, qualifier: &str, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(format!("{qualifier}{}", service.name_of(id)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    member,
                    field(&member.name),
                    qualified_yard_type(service, &member.target, qualifier),
                )
            })
            .collect();
    }
    structure
}
