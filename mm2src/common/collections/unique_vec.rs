use itertools::Itertools;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};
use std::hash::Hash;
use std::ops::Deref;

/// `Vec<T>` that is guarantee to consist of unique items **only**.
#[derive(Debug, Default, Eq, PartialEq, Serialize)]
pub struct UniqueVec<T>(Vec<T>);

impl<T> UniqueVec<T> {
    /// Returns [`Ok(UniqueVec<T>)`] from the raw `Vec<T>` if the `data` consists of unique items,
    /// otherwise returns [`Err(Vec<T>)`].
    pub fn new(data: Vec<T>) -> Result<UniqueVec<T>, Vec<T>>
    where
        T: Eq + Hash,
    {
        if data.iter().all_unique() {
            Ok(UniqueVec(data))
        } else {
            Err(data)
        }
    }

    /// Returns `UniqueVec<T>` from the `I` iterator removing repeating items.
    pub fn new_dedup<I>(i: I) -> UniqueVec<T>
    where
        T: Clone + Eq + Hash,
        I: IntoIterator<Item = T>,
    {
        UniqueVec(i.into_iter().unique().collect())
    }
}

impl<T> AsRef<[T]> for UniqueVec<T> {
    fn as_ref(&self) -> &[T] { &self.0 }
}

impl<T> Deref for UniqueVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target { &self.0 }
}

impl<T> From<UniqueVec<T>> for Vec<T> {
    fn from(orig: UniqueVec<T>) -> Self { orig.0 }
}

impl<T> IntoIterator for UniqueVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter { self.0.into_iter() }
}

/// The `Deserialize` implementation is similar as for `Vec<T>`,
/// but unlike `Vec<T>`, `UniqueVec` checks if the given sequence consists of unique items **only**.
impl<'de, T> Deserialize<'de> for UniqueVec<T>
where
    T: Deserialize<'de> + Eq + Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data = Vec::deserialize(deserializer)?;
        if data.iter().all_unique() {
            Ok(UniqueVec(data))
        } else {
            Err(D::Error::custom("the sequence is expected to consist of unique items"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json as json;

    #[test]
    fn test_unique_vec_deserialize() {
        let data: UniqueVec<i32> = json::from_str("[1, 2, 0, -10, 12]").unwrap();
        assert_eq!(data, UniqueVec(vec![1, 2, 0, -10, 12]));
        let data: UniqueVec<i32> = json::from_str("[1]").unwrap();
        assert_eq!(data, UniqueVec(vec![1]));
        let data: UniqueVec<i32> = json::from_str("[]").unwrap();
        assert_eq!(data, UniqueVec::default());

        json::from_str::<UniqueVec<i32>>("[1, 2, 1, -10, 12]")
            .expect_err("Deserializing should have failed due to repeating items");
        json::from_str::<UniqueVec<i32>>("[1, 1]")
            .expect_err("Deserializing should have failed due to repeating items");
        json::from_str::<UniqueVec<i32>>("[1, 1, 1, 1]")
            .expect_err("Deserializing should have failed due to repeating items");
    }
}
