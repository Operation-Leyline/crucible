//! FnHandler: a handler made from a closure.

use crate::error::Result;
use crate::handler::{Handler, Hook, Job};

type BeforeFn = Box<dyn Fn(&mut Job) -> Result<()> + Send + Sync>;
type AfterFn = Box<dyn Fn(&mut Job) + Send + Sync>;

/// A handler from a synchronous closure: header checks, tagging, deadlines.
/// Implement [`Handler`] for anything that awaits.
pub struct FnHandler {
    before: Option<BeforeFn>,
    after: Option<AfterFn>,
}

pub fn before_fn(before: impl Fn(&mut Job) -> Result<()> + Send + Sync + 'static) -> FnHandler {
    FnHandler {
        before: Some(Box::new(before)),
        after: None,
    }
}

pub fn after_fn(after: impl Fn(&mut Job) + Send + Sync + 'static) -> FnHandler {
    FnHandler {
        before: None,
        after: Some(Box::new(after)),
    }
}

impl FnHandler {
    /// Adds an `after` to a handler made by [`before_fn`].
    pub fn and_after(mut self, after: impl Fn(&mut Job) + Send + Sync + 'static) -> Self {
        self.after = Some(Box::new(after));
        self
    }
}

impl Handler for FnHandler {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(match &self.before {
            Some(before) => before(job),
            None => Ok(()),
        })
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        if let Some(after) = &self.after {
            after(job);
        }
        Hook::Ready(())
    }
}
