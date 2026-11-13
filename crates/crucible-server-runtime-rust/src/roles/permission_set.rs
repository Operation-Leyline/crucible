use std::borrow::Cow;
use std::hash::{Hash, Hasher};

/// A set of a catalogue's permissions, as a bitset: bit `i` is
/// [`RoleCatalogue::permissions`](super::RoleCatalogue::permissions)`[i]`,
/// which is the generated `Permission::ALL` order. Generated catalogues
/// hold `'static` words; a custom role's set (from
/// [`RoleCatalogue::permission_set`](super::RoleCatalogue::permission_set))
/// owns its own. Words past the end are zero, so two sets that differ only
/// in trailing zero words are equal.
#[derive(Debug, Clone, Default)]
pub struct PermissionSet(Cow<'static, [u64]>);

impl PermissionSet {
    /// No permission.
    pub const EMPTY: Self = Self(Cow::Borrowed(&[]));

    /// The set whose bit `i % 64` of word `i / 64` is set for each
    /// permission `i` it holds: what generated catalogues state.
    pub const fn from_words(words: &'static [u64]) -> Self {
        Self(Cow::Borrowed(words))
    }

    /// The set holding exactly the permissions at `bits`.
    pub fn with_bits(bits: impl IntoIterator<Item = usize>) -> Self {
        let mut set = Self::EMPTY;
        for bit in bits {
            set.insert(bit);
        }
        set
    }

    /// Whether it holds permission `bit`.
    #[inline]
    pub fn contains(&self, bit: usize) -> bool {
        self.0
            .get(bit / 64)
            .is_some_and(|word| word >> (bit % 64) & 1 == 1)
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|word| *word == 0)
    }

    /// Adds permission `bit`.
    pub fn insert(&mut self, bit: usize) {
        let words = self.0.to_mut();
        if words.len() <= bit / 64 {
            words.resize(bit / 64 + 1, 0);
        }
        words[bit / 64] |= 1 << (bit % 64);
    }

    /// Adds every permission of `other`.
    pub fn union_with(&mut self, other: &PermissionSet) {
        if other.is_empty() {
            return;
        }
        let words = self.0.to_mut();
        if words.len() < other.0.len() {
            words.resize(other.0.len(), 0);
        }
        for (word, theirs) in words.iter_mut().zip(other.0.iter()) {
            *word |= theirs;
        }
    }

    /// The permissions both hold.
    pub fn intersection(&self, other: &PermissionSet) -> PermissionSet {
        Self(Cow::Owned(
            self.0
                .iter()
                .zip(other.0.iter())
                .map(|(a, b)| a & b)
                .collect(),
        ))
    }

    /// The permissions it holds, by bit, ascending.
    pub fn bits(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(index, word)| {
            (0..64)
                .filter(move |bit| word >> bit & 1 == 1)
                .map(move |bit| index * 64 + bit)
        })
    }

    /// Its words, as [`from_words`](Self::from_words) takes them.
    pub fn words(&self) -> &[u64] {
        &self.0
    }

    /// The words without trailing zero ones: what equality compares.
    fn significant(&self) -> &[u64] {
        let end = self
            .0
            .iter()
            .rposition(|word| *word != 0)
            .map_or(0, |last| last + 1);
        &self.0[..end]
    }
}

impl PartialEq for PermissionSet {
    fn eq(&self, other: &Self) -> bool {
        self.significant() == other.significant()
    }
}

impl Eq for PermissionSet {}

impl Hash for PermissionSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.significant().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::PermissionSet;

    #[test]
    fn a_set_reads_its_bits_across_words() {
        static WORDS: [u64; 2] = [0b101, 1 << 3];
        let set = PermissionSet::from_words(&WORDS);
        assert!(set.contains(0) && !set.contains(1) && set.contains(2));
        assert!(set.contains(67) && !set.contains(64));
        assert!(!set.contains(500));
        assert_eq!(set.bits().collect::<Vec<_>>(), [0, 2, 67]);
        assert_eq!(set, PermissionSet::with_bits([67, 2, 0]));
        assert_ne!(set, PermissionSet::with_bits([0, 2]));
    }

    #[test]
    fn trailing_zero_words_do_not_matter() {
        static WORDS: [u64; 3] = [0b1, 0, 0];
        assert_eq!(
            PermissionSet::from_words(&WORDS),
            PermissionSet::with_bits([0])
        );
        assert_eq!(PermissionSet::from_words(&[0, 0]), PermissionSet::EMPTY);
        assert!(PermissionSet::from_words(&[0, 0]).is_empty());
    }

    #[test]
    fn union_and_intersection() {
        let mut a = PermissionSet::with_bits([1, 70]);
        let b = PermissionSet::with_bits([1, 2]);
        assert_eq!(a.intersection(&b), PermissionSet::with_bits([1]));
        a.union_with(&b);
        assert_eq!(a, PermissionSet::with_bits([1, 2, 70]));
    }
}
