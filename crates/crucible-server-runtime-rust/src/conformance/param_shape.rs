//! Smithy `params` are written the same way for every protocol: blobs as
//! plain text, timestamps as epoch seconds, members by their Smithy names.
//! The generated model reads and writes them in its own wire form. A
//! `ParamShape` describes, from the model, the few places where the two
//! differ, so the harness can translate and then compare typed values.

use serde_json::{Map, Value};

use crate::blob::Blob;
use crate::operation::TimestampFormat;
use crate::time;

/// A model shape, as far as `params` translation needs it.
pub enum ParamShape {
    /// Written the same way in `params` and on the wire: strings, numbers,
    /// booleans, enums, documents.
    Plain,
    /// Plain text in `params`; base64 in the model's documents.
    Blob,
    /// A number in `params`; its digits as a string in the model's documents,
    /// as protocols that carry arbitrary-precision numbers as text write them.
    NumberText,
    /// Epoch seconds or text in `params`; this format in the model's documents.
    Timestamp(TimestampFormat),
    List(&'static ParamShape),
    Map(&'static ParamShape),
    Structure(&'static [ParamMember]),
    /// One member, named, holding its value.
    Union(&'static [ParamMember]),
}

/// A structure or union member.
pub struct ParamMember {
    /// The member's name in the model, which `params` use.
    pub name: &'static str,
    /// Its name in the model's documents.
    pub wire: &'static str,
    pub shape: &'static ParamShape,
}

impl ParamShape {
    /// `params` rewritten the way the generated type reads documents.
    pub fn translate(&self, value: &Value) -> Result<Value, String> {
        self.walk(value, &mut String::new())
    }

    fn walk(&self, value: &Value, path: &mut String) -> Result<Value, String> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        match self {
            Self::Plain => Ok(value.clone()),
            Self::Blob => match value {
                Value::String(text) => Ok(Value::String(Blob::new(text.clone()).to_base64())),
                _ => Err(at(path, "a blob is written as text in params")),
            },
            Self::NumberText => match value {
                Value::Number(number) => Ok(Value::String(number.to_string())),
                Value::String(_) => Ok(value.clone()),
                _ => Err(at(path, "a number is expected")),
            },
            Self::Timestamp(format) => {
                let timestamp = time::from_json(value)
                    .ok_or_else(|| at(path, "not a timestamp: epoch seconds or text expected"))?;
                time::to_json(&timestamp, *format).map_err(|message| at(path, &message))
            }
            Self::List(item) => {
                let Value::Array(items) = value else {
                    return Err(at(path, "a list is expected"));
                };
                items
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        scoped(path, &format!("[{index}]"), |path| item.walk(element, path))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            }
            Self::Map(entry) => {
                let Value::Object(entries) = value else {
                    return Err(at(path, "a map is expected"));
                };
                let mut translated = Map::new();
                for (key, element) in entries {
                    let walked =
                        scoped(path, &format!(".{key}"), |path| entry.walk(element, path))?;
                    translated.insert(key.clone(), walked);
                }
                Ok(Value::Object(translated))
            }
            Self::Structure(members) | Self::Union(members) => {
                let Value::Object(entries) = value else {
                    return Err(at(path, "a structure is expected"));
                };
                let mut translated = Map::new();
                for (key, element) in entries {
                    let member = members
                        .iter()
                        .find(|member| member.name == key)
                        .ok_or_else(|| {
                            at(path, &format!("`{key}` is not a member of this shape"))
                        })?;
                    let walked = scoped(path, &format!(".{key}"), |path| {
                        member.shape.walk(element, path)
                    })?;
                    translated.insert(member.wire.to_owned(), walked);
                }
                Ok(Value::Object(translated))
            }
        }
    }
}

fn scoped<T>(
    path: &mut String,
    segment: &str,
    f: impl FnOnce(&mut String) -> Result<T, String>,
) -> Result<T, String> {
    let length = path.len();
    path.push_str(segment);
    let result = f(path);
    path.truncate(length);
    result
}

fn at(path: &str, message: &str) -> String {
    if path.is_empty() {
        format!("params: {message}")
    } else {
        format!("params{path}: {message}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    static PLAIN: ParamShape = ParamShape::Plain;
    static BLOB: ParamShape = ParamShape::Blob;
    static DATE_TIME: ParamShape = ParamShape::Timestamp(TimestampFormat::DateTime);
    static EPOCH: ParamShape = ParamShape::Timestamp(TimestampFormat::EpochSeconds);
    static HTTP_DATE: ParamShape = ParamShape::Timestamp(TimestampFormat::HttpDate);
    static NUMBER_TEXT: ParamShape = ParamShape::NumberText;
    static BLOBS: ParamShape = ParamShape::List(&BLOB);
    static BY_NAME: ParamShape = ParamShape::Map(&DATE_TIME);
    static CHOICE: ParamShape = ParamShape::Union(&[ParamMember {
        name: "when",
        wire: "At",
        shape: &EPOCH,
    }]);
    static RECORD: ParamShape = ParamShape::Structure(&[
        ParamMember {
            name: "title",
            wire: "t",
            shape: &PLAIN,
        },
        ParamMember {
            name: "data",
            wire: "d",
            shape: &BLOB,
        },
        ParamMember {
            name: "made",
            wire: "m",
            shape: &DATE_TIME,
        },
        ParamMember {
            name: "sent",
            wire: "s",
            shape: &HTTP_DATE,
        },
        ParamMember {
            name: "many",
            wire: "many",
            shape: &BLOBS,
        },
        ParamMember {
            name: "named",
            wire: "named",
            shape: &BY_NAME,
        },
        ParamMember {
            name: "choice",
            wire: "choice",
            shape: &CHOICE,
        },
        ParamMember {
            name: "big",
            wire: "big",
            shape: &NUMBER_TEXT,
        },
    ]);

    #[test]
    fn members_blobs_and_timestamps_are_written_the_way_the_model_reads_them() {
        let mut params = json!({
            "title": "x",
            "data": "foo",
            "made": 1398796238,
            "sent": 1398796238,
            "many": ["a", "b"],
            "named": {"k": "2014-04-29T18:30:38Z"},
            "choice": {"when": 1.5},
        });
        // More digits than a float keeps.
        params["big"] = serde_json::from_str("0.100000000000000000000001").unwrap();
        let translated = RECORD.translate(&params).unwrap();
        assert_eq!(
            translated,
            json!({
                "t": "x",
                "d": "Zm9v",
                "m": "2014-04-29T18:30:38Z",
                "s": "Tue, 29 Apr 2014 18:30:38 GMT",
                "many": ["YQ==", "Yg=="],
                "named": {"k": "2014-04-29T18:30:38Z"},
                "choice": {"At": 1.5},
                "big": "0.100000000000000000000001",
            })
        );
    }

    #[test]
    fn nulls_pass_through_and_mistakes_name_their_path() {
        assert_eq!(
            RECORD.translate(&json!({"data": null})).unwrap(),
            json!({"d": null})
        );
        for (params, expected) in [
            (json!({"nope": 1}), "params: `nope` is not a member"),
            (json!({"data": 3}), "params.data: a blob is written as text"),
            (json!({"made": "yesterday"}), "params.made: not a timestamp"),
            (json!({"many": ["a", 1]}), "params.many[1]: a blob"),
            (
                json!({"named": {"k": true}}),
                "params.named.k: not a timestamp",
            ),
            (
                json!({"choice": {"other": 1}}),
                "params.choice: `other` is not a member",
            ),
            (json!({"big": true}), "params.big: a number is expected"),
            (json!([]), "params: a structure is expected"),
        ] {
            let error = RECORD.translate(&params).unwrap_err();
            assert!(error.contains(expected), "{params}: {error}");
        }
    }
}
