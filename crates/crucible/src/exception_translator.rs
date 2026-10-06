//! Exception translation: turns internal failures into modelled errors by
//! rules the service registers. What no rule translates stays an internal
//! failure, answered with no detail.

use crate::error::{Error, MAX_SOURCE_DEPTH};
use crate::{Job, Resolution};

type Rule = Box<dyn Fn(&(dyn std::error::Error + 'static)) -> Option<Error> + Send + Sync>;

#[derive(Default)]
pub struct ExceptionTranslator {
    rules: Vec<Rule>,
}

impl ExceptionTranslator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Translates an `E` found anywhere in a failure's source chain.
    pub fn translate<E: std::error::Error + 'static>(
        self,
        rule: impl Fn(&E) -> Error + Send + Sync + 'static,
    ) -> Self {
        self.translate_with(move |error| error.downcast_ref::<E>().map(&rule))
    }

    /// A rule over any error, for matching more than a type.
    pub fn translate_with(
        mut self,
        rule: impl Fn(&(dyn std::error::Error + 'static)) -> Option<Error> + Send + Sync + 'static,
    ) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    fn translation(&self, error: &Error) -> Option<Error> {
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error.get_ref());
        for _ in 0..=MAX_SOURCE_DEPTH {
            let Some(candidate) = current else { break };
            if let Some(translated) = self.rules.iter().find_map(|rule| rule(candidate)) {
                return Some(translated);
            }
            current = candidate.source();
        }
        None
    }
}

impl ExceptionTranslator {
    pub(crate) fn apply(&self, job: &mut Job) {
        let Some(Resolution::Internal(error)) = job.resolution() else {
            return;
        };
        if let Some(translated) = self.translation(error) {
            tracing::debug!(
                request_id = job.context().request_id(),
                error = error.chain(),
                "translated an internal failure"
            );
            job.replace_error(translated);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    #[derive(Debug)]
    struct CustomError(&'static str);

    impl std::fmt::Display for CustomError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for CustomError {}

    #[derive(Debug)]
    struct NestedError {
        msg: &'static str,
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    }

    impl std::fmt::Display for NestedError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.msg)
        }
    }

    impl std::error::Error for NestedError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.source
                .as_deref()
                .map(|s| s as &(dyn std::error::Error + 'static))
        }
    }

    #[derive(Debug)]
    struct CyclicError;

    impl std::fmt::Display for CyclicError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("loop")
        }
    }

    impl std::error::Error for CyclicError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(self)
        }
    }

    #[test]
    fn translates_root_error() {
        let translator = ExceptionTranslator::new()
            .translate(|e: &CustomError| Error::internal(format!("translated: {}", e.0)));
        let error = Error::new(CustomError("root"));
        let translated = translator.translation(&error).expect("should translate");
        assert_eq!(translated.to_string(), "translated: root");
    }

    #[test]
    fn translates_nested_error() {
        let translator = ExceptionTranslator::new()
            .translate(|e: &CustomError| Error::internal(format!("translated: {}", e.0)));
        let nested = NestedError {
            msg: "outer",
            source: Some(Box::new(CustomError("inner"))),
        };
        let error = Error::new(nested);
        let translated = translator.translation(&error).expect("should translate");
        assert_eq!(translated.to_string(), "translated: inner");
    }

    #[test]
    fn unhandled_cyclic_error_terminates_safely() {
        let translator = ExceptionTranslator::new()
            .translate(|e: &CustomError| Error::internal(format!("translated: {}", e.0)));
        let error = Error::new(CyclicError);
        assert!(translator.translation(&error).is_none());
    }

    #[test]
    fn matching_cyclic_error_translates() {
        let translator = ExceptionTranslator::new()
            .translate(|_: &CyclicError| Error::internal("recovered from cyclic"));
        let error = Error::new(CyclicError);
        let translated = translator.translation(&error).expect("should translate");
        assert_eq!(translated.to_string(), "recovered from cyclic");
    }

    #[test]
    fn bounds_deep_error_hierarchy() {
        let mut current = NestedError {
            msg: "step",
            source: Some(Box::new(CustomError("deep_target"))),
        };
        for _ in 0..100 {
            current = NestedError {
                msg: "step",
                source: Some(Box::new(current)),
            };
        }
        let translator = ExceptionTranslator::new()
            .translate(|_: &CustomError| Error::internal("translated deep"));
        let error = Error::new(current);
        assert!(translator.translation(&error).is_none());
    }

    #[test]
    fn finds_error_within_bound_in_deep_hierarchy() {
        let mut current = NestedError {
            msg: "step",
            source: Some(Box::new(CustomError("target_at_64"))),
        };
        for _ in 0..63 {
            current = NestedError {
                msg: "step",
                source: Some(Box::new(current)),
            };
        }
        let translator = ExceptionTranslator::new()
            .translate(|_: &CustomError| Error::internal("translated at bound"));
        let error = Error::new(current);
        assert!(translator.translation(&error).is_some());
    }
}
