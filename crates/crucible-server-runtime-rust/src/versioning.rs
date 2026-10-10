//! Serving older API versions from the current implementation
//! (docs/api-versions.md). A request names its version in the
//! `Crucible-Version` header; an older version is routed and decoded with
//! its own release's types, converted step by step to the current input,
//! handled, and its output or error converted back before encoding. The
//! conversions are `'static` data generated from the model's migrations;
//! this module interprets them.

use std::sync::Arc;

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
    pub const fn new(order: u16, name: &'static str) -> Self {
        Self { order, name }
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

/// Applies `transforms` in order.
pub(crate) fn apply(value: &mut Value, transforms: &[Transform]) {
    for transform in transforms {
        let (at, last) = match transform {
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

/// The input of an older version, read as the current version's.
pub(crate) type Upgrade =
    Arc<dyn Fn(Box<dyn Input>) -> Result<Box<dyn Input>, Rejection> + Send + Sync>;
/// The current version's output, read as an older version's.
pub(crate) type Downgrade = Arc<
    dyn Fn(Box<dyn crate::operation::Output>) -> crate::Result<Box<dyn crate::operation::Output>>
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

/// `YYYY-MM-DD` as an HTTP date, for the `Sunset` header.
pub(crate) fn http_date(date: &str) -> Option<String> {
    let mut parts = date.split('-');
    let (year, month, day): (i64, u32, u32) = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01, civil from days (Howard Hinnant).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    Some(format!(
        "{weekday}, {day:02} {} {year} 00:00:00 GMT",
        months[month as usize - 1]
    ))
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
        assert_eq!(http_date("soon"), None);
    }
}
