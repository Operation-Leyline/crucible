use crate::OperationMetadata;
use crate::error::{Error, Result};
use crate::operation::{Input, Operation, Output, Segment, ServiceInfo};
use crate::operations::{
    Entry, Invoke, MissingOperations, OlderVersion, Operations, OutputFromJson, StreamingOutput,
    decode, decode_bound, decode_cbor, decode_events, decode_json, output_from_json,
};
use crate::validate::Validator;
use crate::versioning::{
    self, ApiVersion, Conversion, Downgrade, DowngradeError, HookFn, Hooks, Upgrade,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct OperationsBuilder {
    pub(crate) operations: Operations,
    /// Older API versions, oldest first, with their sunsets and releases.
    pub(crate) older: Vec<(ApiVersion, Option<&'static str>, &'static ServiceInfo)>,
    /// Older versions' operations, built once the current ones are known.
    pub(crate) versioned: Vec<Versioned>,
    /// The service's migration hooks, by name.
    pub(crate) hooks: Hooks,
    /// Handlers of operations a later version removed, by the last version
    /// that had each.
    pub(crate) legacy: BTreeMap<(ApiVersion, &'static str), Entry>,
}

/// An older version's operation: its entry is built from the operation
/// that serves it, which may be registered after it.
pub(crate) struct Versioned {
    version: ApiVersion,
    name: &'static str,
    /// Not served at this version: a stream.
    refused: bool,
    /// Served by the handler of the last version that had the operation,
    /// named for a missing-handler report; otherwise by the current one.
    served_by: Option<(ApiVersion, &'static str)>,
    /// The hooks its conversions call.
    hooks: Vec<&'static str>,
    entry: BuildEntry,
}

/// Builds an older version's entry from the entry that serves it.
type BuildEntry = Box<dyn Fn(&Entry, &'static ServiceInfo, &Arc<Hooks>) -> Entry + Send + Sync>;

/// The entry for `O`, served by `call` on `implementation`.
fn entry<O, S, Fut>(
    implementation: Arc<S>,
    call: impl Fn(Arc<S>, O::Input) -> Fut + Send + Sync + 'static,
) -> Entry
where
    O: Operation,
    S: Send + Sync + 'static,
    Fut: Future<Output = Result<O::Output>> + Send + 'static,
{
    let invoke: Invoke = Arc::new(move |input: Box<dyn Input>| {
        let input = match input.into_any().downcast::<O::Input>() {
            Ok(input) => *input,
            Err(_) => {
                let error = Error::internal(format!("{}: input of the wrong type", O::INFO.name));
                return Box::pin(std::future::ready(Err(error)));
            }
        };
        let output = call(implementation.clone(), input);
        Box::pin(
            async move { Ok(Box::new(StreamingOutput::<O>(output.await?)) as Box<dyn Output>) },
        )
    });
    Entry {
        info: O::INFO,
        metadata: OperationMetadata::of::<O>(),
        decode: decode::<O>,
        decode_json: decode_json::<O>,
        decode_cbor: decode_cbor::<O>,
        decode_bound: decode_bound::<O>,
        decode_events: decode_events::<O>,
        invoke,
        output_from_json: O::OUTPUT_FROM_JSON.map(|_| output_from_json::<O> as OutputFromJson),
        body_limit: None,
        upgrade: None,
        downgrade: None,
        downgrade_error: None,
        errors: None,
    }
}

impl OperationsBuilder {
    /// Registers `O`, served by `call` on `implementation`. Generated code
    /// calls this; a service registers through the generated setters.
    ///
    /// `call` returns the implementation's future as it is; the framework
    /// boxes it once, together with turning its output into the response.
    pub fn add<O, S, Fut>(
        mut self,
        implementation: Arc<S>,
        call: impl Fn(Arc<S>, O::Input) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        O: Operation,
        S: Send + Sync + 'static,
        Fut: Future<Output = Result<O::Output>> + Send + 'static,
    {
        self.operations
            .entries
            .insert(O::INFO.name, entry::<O, S, Fut>(implementation, call));
        self
    }

    /// Names the current API version and the older ones still served,
    /// oldest first, with their sunsets and their releases' service
    /// descriptions, which their routes are read against. Generated for a
    /// model with migrations (docs/api-versions.md).
    #[doc(hidden)]
    pub fn versions(
        mut self,
        current: ApiVersion,
        older: &'static [(ApiVersion, Option<&'static str>, &'static ServiceInfo)],
    ) -> Self {
        self.operations.versions.current = Some(current);
        self.older = older.to_vec();
        self
    }

    /// Registers a migration hook the service wrote. Generated.
    #[doc(hidden)]
    pub fn hook(mut self, name: &'static str, hook: HookFn) -> Self {
        self.hooks.insert(name, hook);
        self
    }

    /// Registers `O` as `version` declared it, served by `call`: the
    /// handler older versions keep for an operation a later version
    /// removed. Generated.
    #[doc(hidden)]
    pub fn add_legacy<O, S, Fut>(
        mut self,
        version: ApiVersion,
        release: &'static ServiceInfo,
        implementation: Arc<S>,
        call: impl Fn(Arc<S>, O::Input) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        O: Operation,
        S: Send + Sync + 'static,
        Fut: Future<Output = Result<O::Output>> + Send + 'static,
    {
        let mut entry = entry::<O, S, Fut>(implementation, call);
        // It returns its own release's errors.
        entry.errors = Some(release);
        self.legacy.insert((version, O::INFO.name), entry);
        self
    }

    /// Registers `Old`, an operation as an older `version` declared it,
    /// served through `chain` (that version's conversion and every later
    /// one's, oldest first) by `Current`: the current operation, or, with
    /// `served_by`, the legacy handler of the last version that had it.
    /// Generated.
    #[doc(hidden)]
    pub fn add_versioned<Old: Operation, Current: Operation>(
        mut self,
        version: ApiVersion,
        chain: &'static [&'static Conversion],
        served_by: Option<(ApiVersion, &'static str)>,
    ) -> Self
    where
        Old::Output: serde::de::DeserializeOwned,
    {
        // Streams are not converted: such operations are served at the
        // current version only.
        let refused = chain.iter().any(|conversion| conversion.refused.is_some())
            || Current::INPUT_EVENTS.is_some()
            || Current::OUTPUT_EVENTS.is_some();
        let hooks = chain
            .iter()
            .flat_map(|conversion| {
                versioning::hooks_named(conversion.upgrade)
                    .chain(versioning::hooks_named(conversion.downgrade))
            })
            .collect();
        self.versioned.push(Versioned {
            version,
            name: Current::INFO.name,
            refused,
            served_by,
            hooks,
            entry: Box::new(move |current, service, hooks| {
                versioned_entry::<Old, Current>(current, service, chain, hooks.clone())
            }),
        });
        self
    }

    /// The table, or the operations, handlers and hooks the service has not
    /// implemented.
    pub fn build(mut self) -> std::result::Result<Operations, MissingOperations> {
        let service = self.operations.service;
        let hooks = Arc::new(std::mem::take(&mut self.hooks));
        let mut missing: Vec<&'static str> = self
            .operations
            .service
            .operations
            .iter()
            .map(|op| op.name)
            .filter(|name| !self.operations.entries.contains_key(name))
            .collect();
        for (version, sunset, release) in std::mem::take(&mut self.older) {
            let mut operations = Operations::builder(release).operations;
            for versioned in self.versioned.iter().filter(|v| v.version == version) {
                if versioned.refused {
                    continue;
                }
                let serving = match versioned.served_by {
                    None => self.operations.entries.get(versioned.name),
                    Some((at, label)) => {
                        let found = self.legacy.get(&(at, versioned.name));
                        if found.is_none() && !missing.contains(&label) {
                            missing.push(label);
                        }
                        found
                    }
                };
                for hook in &versioned.hooks {
                    if !hooks.contains_key(hook) && !missing.contains(hook) {
                        missing.push(hook);
                    }
                }
                if let Some(serving) = serving {
                    let entry = (versioned.entry)(serving, service, &hooks);
                    operations.entries.insert(entry.info.name, entry);
                }
            }
            index(&mut operations);
            self.operations.versions.older.push(OlderVersion {
                version,
                sunset: sunset
                    .and_then(versioning::http_date)
                    .and_then(|date| axum::http::HeaderValue::from_str(&date).ok()),
                operations,
            });
        }
        index(&mut self.operations);
        if missing.is_empty() {
            Ok(self.operations)
        } else {
            Err(MissingOperations(missing))
        }
    }
}

/// Fills the route index of `operations` from its entries.
fn index(operations: &mut Operations) {
    let infos: Vec<_> = operations
        .entries
        .values()
        .map(|entry| entry.info)
        .collect();
    for info in infos {
        if info
            .path
            .iter()
            .any(|s| matches!(s, Segment::GreedyLabel(_)))
        {
            operations.greedy_routes.push(info);
            continue;
        }
        let routes = &mut operations.routes;
        if routes.len() <= info.path.len() {
            routes.resize_with(info.path.len() + 1, Vec::new);
        }
        routes[info.path.len()].push(info);
    }
}

/// The entry of `Old`, an older version's operation, served by `serving`
/// through `chain`: it decodes as `Old`, converts its input up to
/// `Current`'s, and converts the outcome back down before encoding.
fn versioned_entry<Old: Operation, Current: Operation>(
    serving: &Entry,
    service: &'static ServiceInfo,
    chain: &'static [&'static Conversion],
    hooks: Arc<Hooks>,
) -> Entry
where
    Old::Output: serde::de::DeserializeOwned,
{
    let upgrade: Upgrade = {
        let hooks = hooks.clone();
        Arc::new(move |input: Box<dyn Input>| {
            let hooks = hooks.clone();
            Box::pin(async move {
                let mut value = input.to_json();
                for conversion in chain {
                    versioning::apply_with(&mut value, conversion.upgrade, &hooks).await?;
                }
                let input: Current::Input = crucible_wire::object::from_value(value)
                    .map_err(crate::operations::unreadable)?;
                Validator::check(&input)?;
                Ok(Box::new(input) as Box<dyn Input>)
            })
        })
    };
    let downgrade: Downgrade = Arc::new(move |output: Box<dyn Output>| {
        let hooks = hooks.clone();
        Box::pin(async move {
            let mut value = output.to_json();
            for conversion in chain.iter().rev() {
                versioning::apply_with(&mut value, conversion.downgrade, &hooks).await?;
            }
            let output: Old::Output = serde_json::from_value(value).map_err(|error| {
                Error::internal(format!(
                    "{}: the output does not convert to its older version: {error}",
                    Old::INFO.name
                ))
            })?;
            Ok(Box::new(StreamingOutput::<Old>(output)) as Box<dyn Output>)
        })
    });
    let classify = service.classify;
    // A legacy handler answers with its own release's errors.
    let release = serving.errors.map(|release| release.classify);
    let downgrade_error: DowngradeError = Arc::new(move |mut error: Error| {
        for conversion in chain.iter().rev() {
            for known in conversion.errors {
                let Some(modelled) = release
                    .and_then(|classify| classify(error.get_ref()))
                    .or_else(|| classify(error.get_ref()))
                else {
                    return error;
                };
                if modelled.shape_id() != known.from {
                    continue;
                }
                let members = modelled.to_json();
                let mut older = serde_json::Map::new();
                for (to, from) in known.members {
                    if let Some(value) = members.get(*from) {
                        older.insert((*to).to_owned(), value.clone());
                    }
                }
                // The step names every member the older error requires
                // (the guard checks), so this fails only on a value the
                // older error refuses.
                error = (known.to)(serde_json::Value::Object(older)).unwrap_or_else(|| {
                    Error::internal(format!(
                        "{}: {} does not convert to the error its API version knows",
                        Old::INFO.name,
                        known.from
                    ))
                });
            }
        }
        error
    });
    Entry {
        info: Old::INFO,
        // The interceptors see the current version's input, and a replay
        // reads the current version's output, which converts down as the
        // operation's own would. A version whose model had no token cannot
        // send one, so its requests run unclaimed. Requirements bind the
        // current input, so they are the current version's too, and so are
        // the pagination members: an older request's token arrives under
        // the current member's name, and a version from before the
        // operation was paginated still reaches an operation that is.
        metadata: OperationMetadata {
            pagination: serving.metadata.pagination,
            idempotency_token: serving.metadata.idempotency_token,
            idempotency_token_sent: Old::IDEMPOTENCY_TOKEN.is_some()
                && serving.metadata.idempotency_token.is_some(),
            replays_output: serving.metadata.replays_output,
            requirements: serving.metadata.requirements,
            checks: serving.metadata.checks,
            public: serving.metadata.public,
            ..OperationMetadata::of::<Old>()
        },
        decode: decode::<Old>,
        decode_json: decode_json::<Old>,
        decode_cbor: decode_cbor::<Old>,
        decode_bound: decode_bound::<Old>,
        decode_events: decode_events::<Old>,
        invoke: serving.invoke.clone(),
        output_from_json: serving.output_from_json,
        body_limit: serving.body_limit,
        upgrade: Some(upgrade),
        downgrade: Some(downgrade),
        downgrade_error: Some(downgrade_error),
        errors: serving.errors,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crucible_wire::__generated::{operation_info, service_info};

    use super::{entry, versioned_entry};
    use crate::error::ModelledError;
    use crate::operation::{
        Operation, OperationInfo, OutputFromJson, Pagination, PaginationMember, ServiceInfo, Unit,
    };
    use crate::permissions::{Checks, Requirements};

    static INFO: OperationInfo = operation_info(
        "t#Create",
        "Create",
        None,
        "POST",
        &[],
        &[],
        200,
        &[],
        &[],
        &[],
        false,
        false,
        false,
        None,
        &[],
    );

    fn classify<'a>(_: &'a (dyn std::error::Error + 'static)) -> Option<&'a dyn ModelledError> {
        None
    }

    static SERVICE: ServiceInfo = service_info(
        "t#Service",
        "v1",
        None,
        None,
        &[&INFO],
        &[],
        classify,
        None,
        None,
        None,
    );

    const TOKEN: PaginationMember = PaginationMember {
        name: "token",
        wire_name: "token",
    };

    /// The current version, which has the token and declares its
    /// requirements.
    struct Current;
    impl Operation for Current {
        const INFO: &'static OperationInfo = &INFO;
        const REQUIREMENTS: Requirements = Requirements::None;
        const PUBLIC: bool = true;
        const CHECKS: Option<Checks<Unit>> = Some(|_, _| {});
        const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
        const OUTPUT_FROM_JSON: Option<OutputFromJson<Unit>> = Some(serde_json::from_value::<Unit>);
        type Input = Unit;
        type Output = Unit;
    }

    /// An older version that had the token too.
    struct Tokened;
    impl Operation for Tokened {
        const INFO: &'static OperationInfo = &INFO;
        const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
        type Input = Unit;
        type Output = Unit;
    }

    /// An older version from before the token was added.
    struct Untokened;
    impl Operation for Untokened {
        const INFO: &'static OperationInfo = &INFO;
        type Input = Unit;
        type Output = Unit;
    }

    #[test]
    fn an_older_version_without_the_token_runs_unclaimed() {
        let current = entry::<Current, (), _>(Arc::new(()), |_, _| std::future::ready(Ok(Unit {})));
        assert_eq!(current.metadata.idempotency_token, Some(TOKEN));
        assert!(current.metadata.idempotency_token_sent);
        assert!(current.metadata.replays_output);

        let before = versioned_entry::<Untokened, Current>(&current, &SERVICE, &[], Arc::default());
        // The decoded input is the current version's, so the member is the
        // current one; but this version's clients cannot send it.
        assert_eq!(before.metadata.idempotency_token, Some(TOKEN));
        assert!(!before.metadata.idempotency_token_sent);

        let since = versioned_entry::<Tokened, Current>(&current, &SERVICE, &[], Arc::default());
        assert!(since.metadata.idempotency_token_sent);
        // A replay reads the current output, which converts down as the
        // operation's own would.
        assert!(since.metadata.replays_output);
        assert!(since.output_from_json.is_some());
    }

    /// The current version pages by `pageToken`.
    struct Paged;
    impl Operation for Paged {
        const INFO: &'static OperationInfo = &INFO;
        const PAGINATION: Option<Pagination> = Some(Pagination {
            input_token: PaginationMember {
                name: "pageToken",
                wire_name: "pageToken",
            },
            page_size: None,
            input_token_map: false,
        });
        type Input = Unit;
        type Output = Unit;
    }

    /// An older version that called the token `nextToken` (a migration
    /// renamed it).
    struct Renamed;
    impl Operation for Renamed {
        const INFO: &'static OperationInfo = &INFO;
        const PAGINATION: Option<Pagination> = Some(Pagination {
            input_token: PaginationMember {
                name: "nextToken",
                wire_name: "nextToken",
            },
            page_size: None,
            input_token_map: false,
        });
        type Input = Unit;
        type Output = Unit;
    }

    #[test]
    fn an_older_version_pages_by_the_current_members() {
        let current = entry::<Paged, (), _>(Arc::new(()), |_, _| std::future::ready(Ok(Unit {})));
        // The interceptors read the token from the upgraded input, which
        // names it as the current version does.
        let renamed = versioned_entry::<Renamed, Paged>(&current, &SERVICE, &[], Arc::default());
        assert_eq!(renamed.metadata.pagination, Paged::PAGINATION);
        // A version from before the operation paged still reaches one that
        // does, whose implementation asks for its cursor.
        let unpaged = versioned_entry::<Untokened, Paged>(&current, &SERVICE, &[], Arc::default());
        assert_eq!(unpaged.metadata.pagination, Paged::PAGINATION);
    }

    #[test]
    fn an_older_version_is_authorized_by_the_current_requirements() {
        let current = entry::<Current, (), _>(Arc::new(()), |_, _| std::future::ready(Ok(Unit {})));
        let before = versioned_entry::<Untokened, Current>(&current, &SERVICE, &[], Arc::default());
        // The interceptors see the current input, so the current version's
        // requirements and bindings authorize it, though the older version
        // declared none.
        assert_eq!(*before.metadata.requirements, Requirements::None);
        assert!(before.metadata.checks.is_some());
        assert!(before.metadata.public);
        // The operation facts an Authorizer sees (readonly, idempotent) are
        // read from the served version's own description.
        assert!(std::ptr::eq(before.info, Untokened::INFO));
    }
}
