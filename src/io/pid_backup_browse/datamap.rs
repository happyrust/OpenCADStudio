//! What a plant's data map says of its enumerations.
//!
//! `SPPIDDataMap.xml` -- a file of the reference data, 3.7 MB of XML -- maps
//! what the P&ID database stores to what a published drawing says. The part
//! the pinned reader knows is the enumerations: every entry of an
//! enumeration list (`SP_20_3`, the third fluid system) with the value it is
//! published as (`@{...}`), and the names of the lists.
//!
//! The panel shows the file's text as it shows any text; this is the same
//! file read for what it means, as a table under it.

use std::path::Path;

use pid_parse::backup::{read_datamap, DataMap};

/// One entry of an enumeration list and what it is published as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumMapping {
    /// The name of the list the entry is of, when the file names the list.
    pub list: String,
    /// The list's UID: the entry's without its number (`SP_20`).
    pub list_uid: String,
    /// The entry's UID (`SP_20_3`).
    pub uid: String,
    /// The entry's number in its list, when its UID ends in one.
    pub number: Option<i64>,
    /// The value a published drawing carries for it (`@{...}`).
    pub published: String,
}

/// The enumerations of a data map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnumMap {
    /// How many enumeration lists the file names.
    pub lists: usize,
    /// The mapped entries: by list name, then by number.
    pub mappings: Vec<EnumMapping>,
}

/// Whether `name` is a data map the reader understands: `SPPIDDataMap.xml`
/// and the dated copies plants keep beside it (`SPPIDDataMap-20230317.xml`).
pub fn is_data_map(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("sppiddatamap") && lower.ends_with(".xml")
}

/// Read the enumerations of the data map at `path`.
pub fn read_enum_map(path: &Path) -> Result<EnumMap, String> {
    read_datamap(path)
        .map(|map| enum_map(&map))
        .map_err(|error| crate::tf!("cannot read {}: {}", path.display(), error).into_owned())
}

/// The enumerations of `map`, in the order the panel lists them.
pub fn enum_map(map: &DataMap) -> EnumMap {
    let mut mappings: Vec<EnumMapping> = map
        .enum_to_enum
        .iter()
        .map(|(uid, published)| {
            // `SP_20_3` is entry 3 of list `SP_20`.
            let (list_uid, number) = match uid.rsplit_once('_') {
                Some((list, number)) => (list.to_string(), number.parse().ok()),
                None => (String::new(), None),
            };
            EnumMapping {
                list: map
                    .enum_list_names
                    .get(&list_uid)
                    .cloned()
                    .unwrap_or_default(),
                list_uid,
                uid: uid.clone(),
                number,
                published: published.clone(),
            }
        })
        .collect();
    mappings.sort_by(|left, right| {
        left.list
            .to_lowercase()
            .cmp(&right.list.to_lowercase())
            .then_with(|| left.list_uid.cmp(&right.list_uid))
            .then_with(|| left.number.cmp(&right.number))
            .then_with(|| left.uid.cmp(&right.uid))
    });
    EnumMap {
        lists: map.enum_list_names.len(),
        mappings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Container>
   <SPMapEnumListDef>
      <IObject UID="SP_20" Name="Fluid System" Description="Fluid"/>
   </SPMapEnumListDef>
   <SPMapEnumListDef>
      <IObject UID="SP_14" Name="Actuator Type" Description="Actuators"/>
   </SPMapEnumListDef>
   <SPMapEnumDef>
      <IObject UID="SP_14_10" Name="Pneumatic binary" Description="PB"/>
   </SPMapEnumDef>
   <Rel>
      <IObject UID="SP_14_10.SP_14"/>
      <IRel UID1="SP_14" UID2="SP_14_10" DefUID="MapEnumListMapEnum" IsRequired="True"/>
   </Rel>
   <Rel>
      <IObject UID="SP_20_12.F12"/>
      <IRel UID1="SP_20_12" UID2="F12" DefUID="MapEnumToEnum" IsRequired="True"/>
   </Rel>
   <Rel>
      <IObject UID="SP_20_3.F3"/>
      <IRel UID1="SP_20_3" UID2="{0000-F3}" DefUID="MapEnumToEnum" IsRequired="True"/>
   </Rel>
   <Rel>
      <IObject UID="SP_14_10.EE68D"/>
      <IRel UID1="SP_14_10" UID2="EE68D" DefUID="MapEnumToEnum" IsRequired="True"/>
   </Rel>
   <Rel>
      <IObject UID="SP_99_1.X"/>
      <IRel UID1="SP_99_1" UID2="X" DefUID="MapEnumToEnum" IsRequired="True"/>
   </Rel>
</Container>
"#;

    #[test]
    fn the_entries_are_listed_by_list_and_number_with_what_they_publish_as() {
        let folder = std::env::temp_dir().join(format!("ocs-datamap-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("SPPIDDataMap.xml");
        std::fs::write(&path, XML).unwrap();
        let map = read_enum_map(&path).unwrap();
        assert_eq!(map.lists, 2);
        let shown: Vec<(&str, &str, &str, Option<i64>, &str)> = map
            .mappings
            .iter()
            .map(|mapping| {
                (
                    mapping.list.as_str(),
                    mapping.list_uid.as_str(),
                    mapping.uid.as_str(),
                    mapping.number,
                    mapping.published.as_str(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                // A list the file does not name comes first, nameless.
                ("", "SP_99", "SP_99_1", Some(1), "@{X}"),
                ("Actuator Type", "SP_14", "SP_14_10", Some(10), "@{EE68D}"),
                // By number, not by text: 3 before 12.
                ("Fluid System", "SP_20", "SP_20_3", Some(3), "@{0000-F3}"),
                ("Fluid System", "SP_20", "SP_20_12", Some(12), "@{F12}"),
            ]
        );
        // A file that is not there says which.
        let missing = read_enum_map(&folder.join("absent.xml")).unwrap_err();
        assert!(missing.contains("absent.xml"), "{missing}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_data_map_is_told_by_its_name() {
        for name in [
            "SPPIDDataMap.xml",
            "SPPIDDataMap-20230317.xml",
            "SPPIDDataMap - 20230302.xml",
            "sppiddatamap20230223.XML",
        ] {
            assert!(is_data_map(name), "{name}");
        }
        for name in [
            "SPEMDataMap.xml",
            "SPPIDDataMap.mdb",
            "CatalogIndex.xml",
            "My SPPIDDataMap.xml",
        ] {
            assert!(!is_data_map(name), "{name}");
        }
    }
}
