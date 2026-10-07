//! Synchronous checks and outcome transforms, plus raw transport checks.
use crate::{Hook, Job, Middleware, OperationAwareMiddleware, OperationJob, Result};
type Before = Box<dyn Fn(&mut OperationJob<'_>) -> Result<()> + Send + Sync>;
type After = Box<dyn Fn(&mut OperationJob<'_>) + Send + Sync>;
pub struct FnOperationMiddleware {
    before: Option<Before>,
    after: Option<After>,
}
pub fn before_fn(
    before: impl Fn(&mut OperationJob<'_>) -> Result<()> + Send + Sync + 'static,
) -> FnOperationMiddleware {
    FnOperationMiddleware {
        before: Some(Box::new(before)),
        after: None,
    }
}
pub fn after_fn(
    after: impl Fn(&mut OperationJob<'_>) + Send + Sync + 'static,
) -> FnOperationMiddleware {
    FnOperationMiddleware {
        before: None,
        after: Some(Box::new(after)),
    }
}
impl FnOperationMiddleware {
    pub fn and_after(
        mut self,
        after: impl Fn(&mut OperationJob<'_>) + Send + Sync + 'static,
    ) -> Self {
        self.after = Some(Box::new(after));
        self
    }
}
impl OperationAwareMiddleware for FnOperationMiddleware {
    fn before<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, Result<()>> {
        Hook::Ready(self.before.as_ref().map_or(Ok(()), |before| before(job)))
    }
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if let Some(after) = &self.after {
            after(job);
        }
        Hook::Ready(())
    }
}
type RawBefore = Box<dyn Fn(&mut Job) -> Result<()> + Send + Sync>;

pub struct FnMiddleware {
    before: RawBefore,
}
pub fn raw_before_fn(
    before: impl Fn(&mut Job) -> Result<()> + Send + Sync + 'static,
) -> FnMiddleware {
    FnMiddleware {
        before: Box::new(before),
    }
}
impl Middleware for FnMiddleware {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready((self.before)(job))
    }
}
