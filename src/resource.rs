pub use crate::host::resources::ResourceMode;

mod cache;
mod directory;
mod kif;
mod kif_source;
mod router;
mod xml;
pub use crate::host::resources::{ArchiveConfig, ArchiveEntry, Category};
pub use crate::host::storage::FolderPaths;
pub use cache::{CachedResources, ResourceCache};
pub use directory::{ArchiveDirectory, DirectoryEntry};
pub use kif::{KifArchive, verification_seed};
pub use kif_source::KifEntrySource;
pub use router::{
    ArchiveMount, ResourceData, ResourceGroup, ResourceMember, ResourceRouter, ResourceSlot,
    ResourceSource,
};
pub use xml::{XmlDocument, XmlWriter};

pub(crate) fn terminated(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}
