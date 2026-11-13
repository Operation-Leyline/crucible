use super::ScopeRef;
use crate::Result;
use crate::permissions::Assignment;

/// Discharges the operation's `assigns` obligation (docs/roles.md §4.5):
/// asks the Authorizer whether the caller may grant or revoke each of
/// `roles` (role values: the input's for `assigns <member>`, the stored
/// binding's for `assigns loaded`, both for a replacement, see
/// [`replaced`]) at the scope instance `at`, the binding's. When it may not
/// assign one, the request is refused with the operation's 403 (the
/// refusal of its first declared check, which a refusal factory sees under
/// [`Origin::Assignment`](crate::permissions::Origin::Assignment)), and the
/// handler returns that error before writing anything.
///
/// Call it once, before any write: `idempotency::commit()` refuses while
/// it is open, and a success with it open fails as one with an open `by
/// handler` obligation does, so a handler cannot forget it. A request that
/// grants or revokes no role passes none, which closes it.
///
/// `RoleAuthorizer` answers from the catalogue: a role whose `assignedBy`
/// names roles needs a binding of one of them, valid for the caller's kind,
/// whose scope instance covers `at`; a role without `assignedBy`, or a
/// custom role, is whoever the operation's `requires` admitted; a value the
/// catalogue does not know is refused. Another Authorizer answers through
/// [`Authorizer::may_assign`](crate::permissions::Authorizer::may_assign).
///
/// ```ignore
/// // PutRoleBinding: `assigns role at Organization`, a replacement.
/// let stored = bindings.get(&input.organization_id, &input.principal, scope).await?;
/// let before: Vec<&str> = stored.iter().map(|b| b.role.as_str()).collect();
/// let at = ScopeRef::new(scope.as_str(), &identifiers);
/// roles::assign(PutRoleBinding::ASSIGNMENT, roles::replaced(&before, &[input.role.as_str()]), at).await?;
/// idempotency::commit().await?;
/// ```
pub async fn assign<R: AsRef<str>>(
    assignment: Assignment,
    roles: impl IntoIterator<Item = R>,
    at: ScopeRef<'_>,
) -> Result<()> {
    let roles: Vec<String> = roles
        .into_iter()
        .map(|role| role.as_ref().to_owned())
        .collect();
    crate::permissions::assign(assignment, roles, at).await
}

/// The roles a replacement changes, for [`assign`]: each role of `before`
/// that `after` drops (revoked), then each of `after` that `before` lacks
/// (granted), once each. A role kept is neither granted nor revoked, so it
/// is not checked.
pub fn replaced<'a, A: AsRef<str>, B: AsRef<str>>(before: &'a [A], after: &'a [B]) -> Vec<&'a str> {
    let mut changed: Vec<&str> = Vec::new();
    let removed = before
        .iter()
        .map(AsRef::as_ref)
        .filter(|role| !after.iter().any(|kept| kept.as_ref() == *role));
    let added = after
        .iter()
        .map(AsRef::as_ref)
        .filter(|role| !before.iter().any(|held| held.as_ref() == *role));
    for role in removed.chain(added) {
        if !changed.contains(&role) {
            changed.push(role);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::replaced;

    #[test]
    fn a_replacement_changes_what_it_adds_and_what_it_removes() {
        assert_eq!(
            replaced(&["owner", "auditViewer"], &["auditViewer", "graphCreator"]),
            ["owner", "graphCreator"]
        );
        assert!(replaced(&["owner"], &["owner"]).is_empty());
        assert_eq!(replaced::<&str, &str>(&[], &["owner", "owner"]), ["owner"]);
        assert_eq!(replaced::<&str, &str>(&["owner"], &[]), ["owner"]);
    }
}
