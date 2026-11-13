/// What a role grants of one permission, as the catalogue states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grant {
    /// Not at all.
    No,
    /// Wherever a binding of the role covers the target.
    Yes,
    /// `grants .. by service`: the service decides when
    /// ([`BindingSource::decide_by_service`](super::BindingSource::decide_by_service),
    /// behind the `roles` feature).
    ByService,
}
