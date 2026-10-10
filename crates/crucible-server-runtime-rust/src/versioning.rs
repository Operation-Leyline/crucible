//! Serving older API versions from the current implementation
//! (docs/api-versions.md). A request names its version in the
//! `Crucible-Version` header; an older version is routed and decoded with
//! its own release's types, converted step by step to the current input,
//! handled, and its output or error converted back before encoding. The
//! conversions are `'static` data generated from the model's migrations;
//! this module interprets them.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::http::HeaderValue;
use axum::http::request::Parts;
use serde_json::Value;

use crate::error::Error;
use crate::operation::Input;
use crate::validate::Rejection;

/// The request header that names the API version.
pub const VERSION_HEADER: &str = "crucible-version";

/// One API version of the service. Versions order as they were released, so
/// a handler branches on a behaviour change with
/// `self.context().api_version() < Some(V2026_11_01)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    order: u16,
    name: &'static str,
}

impl ApiVersion {
    /// Generated for every version: `order` is its place in the release
    /// chain, oldest first.
    ///
    /// The name travels in a header, so it must be visible ASCII; a
    /// constant with another name fails to compile.
    pub const fn new(order: u16, name: &'static str) -> Self {
        let bytes = name.as_bytes();
        assert!(!bytes.is_empty(), "an API version needs a name");
        let mut index = 0;
        while index < bytes.len() {
            assert!(
                bytes[index].is_ascii_graphic(),
                "an API version's name is visible ASCII"
            );
            index += 1;
        }
        Self { order, name }
    }

    /// The name as a header value, without allocating.
    pub(crate) fn header(&self) -> HeaderValue {
        HeaderValue::from_static(self.name)
    }

    /// The version as the model names it, such as `2026-09-04`.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// Where a value sits in an input's, output's or error's JSON.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    /// A member of a structure.
    Field(&'static str),
    /// Every item of a list.
    Items,
    /// Every value of a map.
    Values,
}

/// One conversion of a JSON value, at every place `at` reaches.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    /// The structure at `at` names member `from` as `to`.
    Rename {
        at: &'static [Seg],
        from: &'static str,
        to: &'static str,
    },
    /// The structure at `at` gets `member` as `value` (JSON text) when it
    /// does not have it: an older request omitting a now-required member.
    Default {
        at: &'static [Seg],
        member: &'static str,
        value: &'static str,
    },
    /// The string at `at` reads `to` where it reads `from`: an enum value
    /// an older client does not know.
    Map {
        at: &'static [Seg],
        from: &'static str,
        to: &'static str,
    },
    /// Every value at `at` becomes what the service's hook `hook` makes of
    /// it: a `reshape` or `retype` step.
    Hook {
        at: &'static [Seg],
        hook: &'static str,
    },
    /// The structure at `at` gets `member` from the service's hook `hook`,
    /// given the structure, when it does not have it: `require .. by hook`.
    Fill {
        at: &'static [Seg],
        member: &'static str,
        hook: &'static str,
    },
}

/// A hook the service wrote, between one version's JSON and the next's.
#[doc(hidden)]
pub type HookFn = Arc<dyn Fn(Value) -> HookFuture + Send + Sync>;
#[doc(hidden)]
pub type HookFuture = Pin<Box<dyn Future<Output = crate::Result<Value>> + Send>>;

/// The hooks a service registered, by name.
pub(crate) type Hooks = BTreeMap<&'static str, HookFn>;

/// A hook's value as `T`, for the generated wrapper around a service's hook.
#[doc(hidden)]
pub fn hook_input<T: serde::de::DeserializeOwned>(hook: &str, value: Value) -> crate::Result<T> {
    serde_json::from_value(value).map_err(|error| {
        Error::internal(format!(
            "API version hook {hook}: unreadable value: {error}"
        ))
    })
}

/// A hook's result as JSON, for the generated wrapper around a service's hook.
#[doc(hidden)]
pub fn hook_output<T: serde::Serialize>(hook: &str, value: T) -> crate::Result<Value> {
    serde_json::to_value(value).map_err(|error| {
        Error::internal(format!(
            "API version hook {hook}: unwritable value: {error}"
        ))
    })
}

/// An error an older version does not know, answered as one it does.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct ErrorDowngrade {
    /// The new error's shape id.
    pub from: &'static str,
    /// Builds the error the older version knows from its members' JSON.
    pub to: fn(Value) -> Option<Error>,
    /// `(older error's member, new error's member)` pairs.
    pub members: &'static [(&'static str, &'static str)],
}

/// How one operation crosses from one version to the next.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct Conversion {
    /// The request, from this version to the next.
    pub upgrade: &'static [Transform],
    /// The response, from the next version back to this one.
    pub downgrade: &'static [Transform],
    pub errors: &'static [ErrorDowngrade],
    /// Why the operation cannot be served at this version yet: a step with
    /// a hook, before hooks are generated. It is then not routed at this
    /// version.
    pub refused: Option<&'static str>,
}

/// The error `E` from its members' JSON: an [`ErrorDowngrade`]'s `to`.
#[doc(hidden)]
pub fn downgrade_to<E>(members: Value) -> Option<Error>
where
    E: serde::de::DeserializeOwned + std::error::Error + Send + Sync + 'static,
{
    serde_json::from_value::<E>(members).ok().map(Error::new)
}

impl Conversion {
    pub const NONE: Conversion = Conversion {
        upgrade: &[],
        downgrade: &[],
        errors: &[],
        refused: None,
    };
}

/// The hooks `transforms` call.
pub(crate) fn hooks_named(transforms: &[Transform]) -> impl Iterator<Item = &'static str> + '_ {
    transforms.iter().filter_map(|transform| match transform {
        Transform::Hook { hook, .. } | Transform::Fill { hook, .. } => Some(*hook),
        _ => None,
    })
}

/// Applies `transforms` in order, calling the service's `hooks`.
pub(crate) async fn apply_with(
    value: &mut Value,
    transforms: &[Transform],
    hooks: &Hooks,
) -> crate::Result<()> {
    let hook = |name: &str| {
        hooks
            .get(name)
            .cloned()
            .ok_or_else(|| Error::internal(format!("API version hook {name} is not registered")))
    };
    for transform in transforms {
        match transform {
            Transform::Hook { at, hook: name } => {
                let call = hook(name)?;
                // Take each value out, convert it, and put it back where it
                // was: the places are visited in the same order both times.
                // A null (an absent optional member) is left alone.
                let mut taken = Vec::new();
                visit(value, at, &mut |slot| {
                    taken.push((!slot.is_null()).then(|| slot.take()));
                });
                let mut converted = Vec::with_capacity(taken.len());
                for item in taken {
                    converted.push(match item {
                        Some(item) => Some(call(item).await?),
                        None => None,
                    });
                }
                let mut converted = converted.into_iter();
                visit(value, at, &mut |slot| {
                    if let Some(Some(item)) = converted.next() {
                        *slot = item;
                    }
                });
            }
            Transform::Fill {
                at,
                member,
                hook: name,
            } => {
                let call = hook(name)?;
                let mut owners = Vec::new();
                visit(value, at, &mut |slot| {
                    if let Value::Object(map) = slot
                        && !map.contains_key(*member)
                    {
                        owners.push(slot.clone());
                    }
                });
                let mut filled = Vec::with_capacity(owners.len());
                for owner in owners {
                    filled.push(call(owner).await?);
                }
                let mut filled = filled.into_iter();
                visit(value, at, &mut |slot| {
                    if let Value::Object(map) = slot
                        && !map.contains_key(*member)
                        && let Some(item) = filled.next()
                    {
                        map.insert((*member).to_owned(), item);
                    }
                });
            }
            plain => apply(value, std::slice::from_ref(plain)),
        }
    }
    Ok(())
}

/// Applies the transforms that need no hook, in order.
pub(crate) fn apply(value: &mut Value, transforms: &[Transform]) {
    for transform in transforms {
        let (at, last) = match transform {
            Transform::Hook { .. } | Transform::Fill { .. } => continue,
            Transform::Rename { at, .. } | Transform::Default { at, .. } => (*at, None),
            Transform::Map { at, .. } => match at.split_last() {
                Some((last, parent)) => (parent, Some(*last)),
                None => (*at, None),
            },
        };
        visit(value, at, &mut |target| match transform {
            Transform::Rename { from, to, .. } => {
                if let Value::Object(map) = target
                    && let Some(moved) = map.remove(*from)
                {
                    map.insert((*to).to_owned(), moved);
                }
            }
            Transform::Default { member, value, .. } => {
                if let Value::Object(map) = target
                    && !map.contains_key(*member)
                    && let Ok(default) = serde_json::from_str(value)
                {
                    map.insert((*member).to_owned(), default);
                }
            }
            Transform::Map { from, to, .. } => {
                let mut swap = |slot: &mut Value| {
                    if slot.as_str() == Some(from) {
                        *slot = Value::String((*to).to_owned());
                    }
                };
                match last {
                    None => swap(target),
                    Some(segment) => visit(target, &[segment], &mut swap),
                }
            }
            Transform::Hook { .. } | Transform::Fill { .. } => {}
        });
    }
}

/// Calls `f` on every value `path` reaches from `value`.
fn visit(value: &mut Value, path: &[Seg], f: &mut dyn FnMut(&mut Value)) {
    let Some((first, rest)) = path.split_first() else {
        f(value);
        return;
    };
    match (first, value) {
        (Seg::Field(name), Value::Object(map)) => {
            if let Some(inner) = map.get_mut(*name) {
                visit(inner, rest, f);
            }
        }
        (Seg::Items, Value::Array(items)) => {
            for item in items {
                visit(item, rest, f);
            }
        }
        (Seg::Values, Value::Object(map)) => {
            for inner in map.values_mut() {
                visit(inner, rest, f);
            }
        }
        _ => {}
    }
}

type Boxed<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send>>;
/// The input of an older version, read as the current version's.
pub(crate) type Upgrade = Arc<dyn Fn(Box<dyn Input>) -> Boxed<Box<dyn Input>> + Send + Sync>;
/// The current version's output, read as an older version's.
pub(crate) type Downgrade = Arc<
    dyn Fn(Box<dyn crate::operation::Output>) -> Boxed<Box<dyn crate::operation::Output>>
        + Send
        + Sync,
>;
/// The current version's error, as an older version knows it.
pub(crate) type DowngradeError = Arc<dyn Fn(Error) -> Error + Send + Sync>;

/// Picks the version of a request that names none.
pub type Resolver = Arc<dyn Fn(&Parts) -> Option<String> + Send + Sync>;

/// How a request's version is settled when it names none.
#[derive(Clone, Default)]
pub enum Unversioned {
    /// Served at the oldest supported version: a client built before it
    /// sent a version was built against an older contract.
    #[default]
    Oldest,
    /// Refused: every caller names its version.
    Required,
    /// Chosen by the service, such as the version an account first used;
    /// `None` falls back to the oldest.
    Resolve(Resolver),
}

/// Why a request's version cannot be served, as a client sees it.
pub(crate) fn unsupported(name: &str, supported: &[ApiVersion]) -> Rejection {
    Rejection::malformed(
        "Crucible-Version",
        format!(
            "the API version \"{name}\" is not supported; this service serves {}",
            supported
                .iter()
                .map(|v| v.name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

pub(crate) fn missing(supported: &[ApiVersion]) -> Rejection {
    Rejection::malformed(
        "Crucible-Version",
        format!(
            "this service requires the API version; it serves {}",
            supported
                .iter()
                .map(|v| v.name)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

/// `YYYY-MM-DD` as an HTTP date, for the `Sunset` header; `None` for a
/// date that does not exist.
pub(crate) fn http_date(date: &str) -> Option<String> {
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(date.format("%a, %d %b %Y 00:00:00 GMT").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transforms_reach_every_place_their_path_names() {
        let mut value = json!({
            "items": [{ "name": "a", "kind": "HUGE" }, { "name": "b", "kind": "SMALL" }],
            "byId": { "x": { "name": "c" } },
        });
        apply(
            &mut value,
            &[
                Transform::Rename {
                    at: &[Seg::Field("items"), Seg::Items],
                    from: "name",
                    to: "title",
                },
                Transform::Rename {
                    at: &[Seg::Field("byId"), Seg::Values],
                    from: "name",
                    to: "title",
                },
                Transform::Map {
                    at: &[Seg::Field("items"), Seg::Items, Seg::Field("kind")],
                    from: "HUGE",
                    to: "LARGE",
                },
                Transform::Default {
                    at: &[],
                    member: "limit",
                    value: "20",
                },
            ],
        );
        assert_eq!(
            value,
            json!({
                "items": [{ "title": "a", "kind": "LARGE" }, { "title": "b", "kind": "SMALL" }],
                "byId": { "x": { "title": "c" } },
                "limit": 20,
            })
        );
        // A default never replaces what the request sent.
        let mut sent = json!({ "limit": 5 });
        apply(
            &mut sent,
            &[Transform::Default {
                at: &[],
                member: "limit",
                value: "20",
            }],
        );
        assert_eq!(sent, json!({ "limit": 5 }));
    }

    #[tokio::test]
    async fn hooks_convert_every_place_and_fill_what_is_missing() {
        let mut hooks = Hooks::new();
        let double: HookFn = Arc::new(|value: Value| {
            Box::pin(async move { Ok(json!(value.as_i64().unwrap_or_default() * 2)) })
        });
        let size: HookFn = Arc::new(|owner: Value| {
            Box::pin(async move { Ok(json!(owner["name"].as_str().unwrap_or("").len())) })
        });
        hooks.insert("double", double);
        hooks.insert("size", size);
        let mut value = json!({
            "items": [{ "name": "abc", "size": 0, "count": null }, { "name": "ab", "count": 1 }],
        });
        apply_with(
            &mut value,
            &[
                Transform::Hook {
                    at: &[Seg::Field("items"), Seg::Items, Seg::Field("count")],
                    hook: "double",
                },
                Transform::Fill {
                    at: &[Seg::Field("items"), Seg::Items],
                    member: "size",
                    hook: "size",
                },
            ],
            &hooks,
        )
        .await
        .unwrap();
        // A null is left alone; a member the request sent is kept.
        assert_eq!(
            value,
            json!({
                "items": [{ "name": "abc", "size": 0, "count": null }, { "name": "ab", "count": 2, "size": 2 }],
            })
        );
        let missing = apply_with(
            &mut value,
            &[Transform::Hook {
                at: &[],
                hook: "absent",
            }],
            &hooks,
        )
        .await;
        assert!(missing.is_err());
    }

    #[test]
    fn versions_order_as_released() {
        const V1: ApiVersion = ApiVersion::new(0, "2026-09-04");
        const V2: ApiVersion = ApiVersion::new(1, "2026-11-01");
        assert!(V1 < V2);
        assert_eq!(V2.name(), "2026-11-01");
    }

    #[test]
    fn sunset_dates_are_http_dates() {
        assert_eq!(
            http_date("2028-11-01").as_deref(),
            Some("Wed, 01 Nov 2028 00:00:00 GMT")
        );
        assert_eq!(
            http_date("1970-01-01").as_deref(),
            Some("Thu, 01 Jan 1970 00:00:00 GMT")
        );
        assert_eq!(http_date("2028-13-01"), None);
        assert_eq!(http_date("2027-02-30"), None);
        assert_eq!(http_date("soon"), None);
    }
}
