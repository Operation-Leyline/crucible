use std::ops::Deref;

use super::Verdict;

/// What [`permissions::decide`](super::decide) answers: one [`Verdict`] per
/// check, in order. Nothing was refused: each verdict that is not
/// [`Verdict::Allowed`] is the handler's to answer, before any write, so
/// dropping them unread (`decide(&checks).await?;`) is a warning. Reads as
/// a slice.
#[must_use = "permissions::decide refuses nothing: answer each verdict that is not Allowed before any write"]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdicts(Vec<Verdict>);

impl Verdicts {
    pub(crate) fn new(verdicts: Vec<Verdict>) -> Self {
        Self(verdicts)
    }

    /// The verdicts, in the checks' order.
    pub fn into_vec(self) -> Vec<Verdict> {
        self.0
    }
}

impl Deref for Verdicts {
    type Target = [Verdict];

    fn deref(&self) -> &[Verdict] {
        &self.0
    }
}

impl IntoIterator for Verdicts {
    type Item = Verdict;
    type IntoIter = std::vec::IntoIter<Verdict>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Verdicts {
    type Item = &'a Verdict;
    type IntoIter = std::slice::Iter<'a, Verdict>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
