use std::collections::BTreeMap;

use crate::ShapeId;
use crate::ir::AuthScheme;
use crate::lower::Lowerer;

impl Lowerer<'_> {
    /// The service's auth schemes: bearer is the only one Crucible implements.
    pub(crate) fn auth_configs(
        &mut self,
        id: &ShapeId,
        schemes: &[ShapeId],
    ) -> BTreeMap<ShapeId, AuthScheme> {
        let mut result = BTreeMap::new();
        for scheme in schemes {
            match (scheme.namespace(), scheme.name()) {
                ("smithy.api", "httpBearerAuth") => {
                    result.insert(scheme.clone(), AuthScheme::Bearer);
                }
                _ => self.diagnostics.push(
                    id,
                    format!("auth scheme {scheme} is not supported; use @httpBearerAuth"),
                ),
            }
        }
        result
    }
}
