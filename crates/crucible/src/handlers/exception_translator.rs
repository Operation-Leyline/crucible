//! Exception translation: turns internal failures into modelled errors by
//! rules the service registers. What no rule translates stays an internal
//! failure, answered with no detail.

use crate::error::Error;
use crate::handler::{Handler, Hook, Job, Resolution};

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
        while let Some(error) = current {
            if let Some(translated) = self.rules.iter().find_map(|rule| rule(error)) {
                return Some(translated);
            }
            current = error.source();
        }
        None
    }
}

impl Handler for ExceptionTranslator {
    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        let Some(Resolution::Internal(error)) = job.resolution() else {
            return Hook::Ready(());
        };
        if let Some(translated) = self.translation(error) {
            tracing::debug!(
                request_id = job.context().request_id(),
                error = error.chain(),
                "translated an internal failure"
            );
            job.replace_error(translated);
        }
        Hook::Ready(())
    }
}
