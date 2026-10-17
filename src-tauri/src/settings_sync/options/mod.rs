//! `options.txt` (spec Part C): a catalog of the keys that sync, their codecs
//! per release, and the key-binding token families.

pub(crate) mod catalog;
pub(crate) mod keys;

#[cfg(test)]
mod evidence;
