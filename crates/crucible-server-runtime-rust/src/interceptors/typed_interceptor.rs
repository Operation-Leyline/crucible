//! Hooks for one operation's concrete input and output types.

use crate::operation::Operation;
use crate::{Context, Error, Prepare, Result};

/// An invocation extension that sees one operation's concrete input and output.
#[async_trait::async_trait]
pub trait TypedInterceptor<O: Operation>: Send + Sync + 'static {
    async fn before(&self, input: &O::Input, context: &Context) -> Result<()> {
        let _ = (input, context);
        Ok(())
    }

    /// `output` is absent when the invocation failed or was answered directly.
    async fn after(&self, output: Option<&O::Output>, context: &Context) {
        let _ = (output, context);
    }

    /// Observes an invocation failure before `after(None, context)` runs.
    async fn failed(&self, error: &Error, context: &Context) {
        let _ = (error, context);
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let _ = prepare;
        Ok(())
    }
}
