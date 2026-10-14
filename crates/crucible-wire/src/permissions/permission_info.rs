use super::{ResourceInfo, Tier};

/// One permission a resource accepts. Generated, `'static`; a direct
/// projection of the IR's `Permission`, with a primary's aliases folded
/// into it rather than listed beside it.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct PermissionInfo {
    pub resource: &'static ResourceInfo,
    /// The model's name for it: `mailSend`.
    pub symbol: &'static str,
    /// Its identity, what grants store and audit records carry with the
    /// resource's name: `mail:send`. Compared case-sensitively.
    pub value: &'static str,
    /// Deprecated values that mean this permission (`email:send`): a grant
    /// stored under one still satisfies a check of this one.
    pub aliases: &'static [&'static str],
    pub tier: Tier,
}

impl PermissionInfo {
    /// Whether `value` is this permission's value or one of its aliases.
    pub fn accepts(&self, value: &str) -> bool {
        self.value == value || self.aliases.contains(&value)
    }

    /// Whether a refusal may name this permission: every tier but
    /// [`Tier::Internal`].
    pub const fn is_public(&self) -> bool {
        self.tier.is_public()
    }
}
