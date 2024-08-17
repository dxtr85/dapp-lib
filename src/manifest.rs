use super::content::DataType;
use gnome::prelude::Data;
use std::collections::HashMap;
pub type ApplicationManifest = HashMap<DataType, String>;

// ApplicationManifest defines application type, data structures, and message headers:
// 0 -> "Catalog" - 0 always defines application type (and optionally more)
// 1 -> "Link" - this defines a data structure containing a Link to a resource
// 2 -> "Text file" - this defines a regular text file data structure
// ...
// 254 -> "RemoveFriend message"
// 255 -> "AddFriend message"
// There can be up to 256 data structures defined in a single application.
// There can be up to 256 synchronization messages defined.
// There can be also some (less than 256) reconfiguration messages defined.
// (We already have some built-in Reconfigs.)

// TODO
pub fn manifest_to_data(manifest: ApplicationManifest) -> Vec<Data> {
    vec![]
}

// TODO
pub fn data_to_manifest(data: Vec<Data>) -> ApplicationManifest {
    let manifest = ApplicationManifest::new();
    manifest
}
