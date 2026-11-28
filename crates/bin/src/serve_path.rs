use std::path::PathBuf;

#[derive(Clone, Debug, derive_more::Deref, derive_more::FromStr)]
#[deref(forward)]
pub(crate) struct ServePath(PathBuf);
