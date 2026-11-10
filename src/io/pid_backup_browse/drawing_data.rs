//! What a drawing's file holds beside its model items.
//!
//! Reading a `.pid` for its items ([`super::load_items`]) reads more than the
//! items: the points where its piping connectors connect, the branch points
//! drawn on its runs, the graphic every item is drawn as, and a tally of how
//! much of the file was understood. The tree kept the items and let go of
//! the rest; [`DrawingData`] is the rest, kept beside them.
//!
//! What only the backup's database knows of a drawing -- its ports, which
//! end of which run they connect, its off-page connectors, its pipelines --
//! is not in the file, and so not here.

use std::collections::HashMap;

use pid_parse::publish::pid_load::{
    PidBranchPoint, PidProcessPoint, PidPublishRepresentation, PidPublishStats, PidValueSource,
};

use super::ItemGroup;

/// What one drawing's file holds beside its items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrawingData {
    /// The points its piping connectors connect at, ordered by UID.
    pub process_points: Vec<ProcessPoint>,
    /// The branch points drawn on its runs, ordered by UID.
    pub branch_points: Vec<BranchPoint>,
    /// The graphic each item is drawn as, ordered by UID.
    pub representations: Vec<Representation>,
    /// How much of the file was read.
    pub stats: ReadStats,
}

impl DrawingData {
    /// Whether the file gave none of it: a drawing with nothing to show
    /// beside its items.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Where a published value comes from, as the reader states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSource {
    /// A label's value row in the file, by its object id.
    LabelRow(u32),
    /// Composed from the item's other values by a naming rule.
    NameRule(String),
    /// Fixed by a rule of the program that published the drawing, at this
    /// address in it.
    NativeRule(String),
    /// A column of a row in the backup's database.
    DatabaseColumn {
        /// The table, e.g. `T_PIPINGPOINT`.
        table: String,
        /// The column, e.g. `PIPINGPOINTNUMBER`.
        column: String,
    },
}

impl From<&PidValueSource> for ValueSource {
    fn from(source: &PidValueSource) -> Self {
        match source {
            PidValueSource::Label { row } => ValueSource::LabelRow(*row),
            PidValueSource::NameRule(rule) => ValueSource::NameRule((*rule).to_string()),
            PidValueSource::DerivedByNativeRule { address, .. } => {
                ValueSource::NativeRule((*address).to_string())
            }
            PidValueSource::BackupRow { table, column } => ValueSource::DatabaseColumn {
                table: (*table).to_string(),
                column: (*column).to_string(),
            },
        }
    }
}

/// A process point: where a piping connector connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessPoint {
    /// The point's UID: the connector's and `.PPT`.
    pub uid: String,
    /// The UID of the piping connector the point is on.
    pub connector_uid: String,
    /// What the tree calls that connector, when it is one of the drawing's
    /// items.
    pub connector: Option<String>,
    /// What fixed the point's UID.
    pub source: ValueSource,
}

/// A branch point drawn on a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchPoint {
    /// The point's UID: the representation's and `.BPT`.
    pub uid: String,
    /// The UID of the representation the point is drawn as.
    pub representation_uid: String,
    /// `PIDPipingBranchPoint` or `PIDSignalBranchPoint`.
    pub class: String,
    /// What fixed the point's UID and class.
    pub source: ValueSource,
}

/// The graphic an item is drawn as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Representation {
    /// The representation's UID (`DrawingID`).
    pub uid: String,
    /// The UID of the item it draws. `None` for a branch point drawn on a
    /// run: that graphic belongs to the branch point, not to the run.
    pub item_uid: Option<String>,
    /// What the tree calls that item, when it is one of the drawing's.
    pub item: Option<String>,
    /// That item's `ModelItemType`.
    pub item_type: Option<String>,
    /// The id of the graphic record in the file: what an imported entity
    /// carries as its `oid`.
    pub graphic_oid: u32,
}

/// How much of a drawing's file was read: [`PidPublishStats`], kept by value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadStats {
    /// Attribute records in the file's record chains.
    pub attribute_records: usize,
    /// Of those, the ones that could not be decoded.
    pub undecoded_records: usize,
    /// Label values that are not blank.
    pub label_values: usize,
    /// Label values that lead to an item.
    pub label_values_reached: usize,
    /// Label values kept as a published attribute.
    pub label_values_published: usize,
    /// Attributes left out because two labels of one item disagree.
    pub conflicting_attributes: usize,
    /// Nominal diameter labels that did not become a published value.
    pub nominal_diameter_not_emitted: usize,
    /// Instruments classed by the folder of their symbol.
    pub instruments_classed_by_library: usize,
}

impl From<PidPublishStats> for ReadStats {
    fn from(stats: PidPublishStats) -> Self {
        ReadStats {
            attribute_records: stats.attribute_records,
            undecoded_records: stats.undecoded_records,
            label_values: stats.label_values,
            label_values_reached: stats.label_values_reached,
            label_values_published: stats.label_values_published,
            conflicting_attributes: stats.conflicting_attributes,
            nominal_diameter_not_emitted: stats.nominal_diameter_not_emitted,
            instruments_classed_by_library: stats.instruments_classed_by_library,
        }
    }
}

/// The rest of a loaded drawing, with the items of `groups` -- labelled for
/// the tree already -- named wherever a row points at one.
pub fn drawing_data(
    process_points: &[PidProcessPoint],
    branch_points: &[PidBranchPoint],
    representations: &[PidPublishRepresentation],
    stats: PidPublishStats,
    groups: &[ItemGroup],
) -> DrawingData {
    // An item's label and type, by its UID.
    let items: HashMap<&str, (&str, &str)> = groups
        .iter()
        .flat_map(|group| {
            group.items.iter().map(|item| {
                (
                    item.uid.as_str(),
                    (item.label.as_str(), group.item_type.as_str()),
                )
            })
        })
        .collect();
    let label = |uid: &str| items.get(uid).map(|(label, _)| (*label).to_string());
    DrawingData {
        process_points: process_points
            .iter()
            .map(|point| ProcessPoint {
                uid: point.uid.clone(),
                connector: label(&point.connector_uid),
                connector_uid: point.connector_uid.clone(),
                source: ValueSource::from(&point.source),
            })
            .collect(),
        branch_points: branch_points
            .iter()
            .map(|point| BranchPoint {
                uid: point.uid.clone(),
                representation_uid: point.representation_uid.clone(),
                class: point.class.to_string(),
                source: ValueSource::from(&point.source),
            })
            .collect(),
        representations: representations
            .iter()
            .map(|representation| {
                let item = representation
                    .item_uid
                    .as_deref()
                    .and_then(|uid| items.get(uid));
                Representation {
                    uid: representation.uid.clone(),
                    item_uid: representation.item_uid.clone(),
                    item: item.map(|(label, _)| (*label).to_string()),
                    item_type: item.map(|(_, item_type)| (*item_type).to_string()),
                    graphic_oid: representation.graphic_oid,
                }
            })
            .collect(),
        stats: ReadStats::from(stats),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::pid_backup_browse::group_items;
    use pid_parse::publish::pid_load::{
        PidPublishAttribute, PidPublishDrawing, PidPublishItem, PipelineSynthesis,
    };

    fn item(uid: &str, item_type: &str, name: Option<&str>) -> PidPublishItem {
        PidPublishItem {
            uid: uid.to_string(),
            model_item_type: item_type.to_string(),
            class: None,
            attributes: name
                .map(|name| PidPublishAttribute {
                    interface: "IObject",
                    name: "Name",
                    value: name.to_string(),
                    source: PidValueSource::Label { row: 7 },
                })
                .into_iter()
                .collect(),
        }
    }

    fn native() -> PidValueSource {
        PidValueSource::DerivedByNativeRule {
            analysis_page: "docs/analysis/points.md",
            address: "0x1003A2B0",
        }
    }

    /// A drawing as the reader hands it over, with nothing from a database.
    fn drawing() -> PidPublishDrawing {
        PidPublishDrawing {
            drawing_uid: "DRAWING-1".to_string(),
            drawing_name: "D01".to_string(),
            items: vec![
                item("RUN-1", "PipeRun", None),
                item("INSTR-1", "Instrument", Some("PT-101")),
            ],
            process_points: vec![PidProcessPoint {
                uid: "RUN-1.PPT".to_string(),
                connector_uid: "RUN-1".to_string(),
                source: native(),
            }],
            branch_points: vec![PidBranchPoint {
                uid: "REP-3.BPT".to_string(),
                representation_uid: "REP-3".to_string(),
                class: "PIDPipingBranchPoint",
                source: native(),
            }],
            ports: Vec::new(),
            connectivity_ends: Vec::new(),
            exp_relations: Vec::new(),
            opc_points: Vec::new(),
            exp: None,
            pipelines: Vec::new(),
            pipelines_attempted: false,
            pipeline_synthesis: PipelineSynthesis::default(),
            representations: vec![
                PidPublishRepresentation {
                    uid: "REP-1".to_string(),
                    item_uid: Some("INSTR-1".to_string()),
                    graphic_oid: 4711,
                },
                PidPublishRepresentation {
                    uid: "REP-3".to_string(),
                    item_uid: None,
                    graphic_oid: 4712,
                },
                // An item the drawing does not publish.
                PidPublishRepresentation {
                    uid: "REP-9".to_string(),
                    item_uid: Some("GONE".to_string()),
                    graphic_oid: 4713,
                },
            ],
            stats: PidPublishStats {
                attribute_records: 120,
                undecoded_records: 2,
                label_values: 30,
                label_values_reached: 28,
                label_values_published: 25,
                conflicting_attributes: 1,
                nominal_diameter_not_emitted: 3,
                instruments_classed_by_library: 4,
            },
        }
    }

    #[test]
    fn the_rest_of_a_drawing_is_kept_with_its_items_named() {
        let items = group_items(drawing());
        assert_eq!(items.item_count, 2);
        let data = &items.data;
        assert!(!data.is_empty());
        assert_eq!(
            data.process_points,
            [ProcessPoint {
                uid: "RUN-1.PPT".to_string(),
                connector_uid: "RUN-1".to_string(),
                // The run publishes no name: the tree's own label for it.
                connector: Some("PipeRun #1".to_string()),
                source: ValueSource::NativeRule("0x1003A2B0".to_string()),
            }]
        );
        assert_eq!(
            data.branch_points,
            [BranchPoint {
                uid: "REP-3.BPT".to_string(),
                representation_uid: "REP-3".to_string(),
                class: "PIDPipingBranchPoint".to_string(),
                source: ValueSource::NativeRule("0x1003A2B0".to_string()),
            }]
        );
        assert_eq!(
            data.representations,
            [
                Representation {
                    uid: "REP-1".to_string(),
                    item_uid: Some("INSTR-1".to_string()),
                    item: Some("PT-101".to_string()),
                    item_type: Some("Instrument".to_string()),
                    graphic_oid: 4711,
                },
                // A branch point drawn on a run names no item.
                Representation {
                    uid: "REP-3".to_string(),
                    item_uid: None,
                    item: None,
                    item_type: None,
                    graphic_oid: 4712,
                },
                // The UID stays when the item is not one of the drawing's.
                Representation {
                    uid: "REP-9".to_string(),
                    item_uid: Some("GONE".to_string()),
                    item: None,
                    item_type: None,
                    graphic_oid: 4713,
                },
            ]
        );
        assert_eq!(
            data.stats,
            ReadStats {
                attribute_records: 120,
                undecoded_records: 2,
                label_values: 30,
                label_values_reached: 28,
                label_values_published: 25,
                conflicting_attributes: 1,
                nominal_diameter_not_emitted: 3,
                instruments_classed_by_library: 4,
            }
        );
    }

    #[test]
    fn a_file_that_gave_nothing_has_no_data() {
        let mut empty = drawing();
        empty.items.clear();
        empty.process_points.clear();
        empty.branch_points.clear();
        empty.representations.clear();
        empty.stats = PidPublishStats::default();
        let items = group_items(empty);
        assert!(items.data.is_empty());
        assert_eq!(items.data, DrawingData::default());
        // One record read is already something to show.
        let mut read = DrawingData::default();
        read.stats.attribute_records = 1;
        assert!(!read.is_empty());
    }

    #[test]
    fn every_kind_of_source_is_kept() {
        assert_eq!(
            ValueSource::from(&PidValueSource::Label { row: 9 }),
            ValueSource::LabelRow(9)
        );
        assert_eq!(
            ValueSource::from(&PidValueSource::NameRule("{a}-{b}")),
            ValueSource::NameRule("{a}-{b}".to_string())
        );
        assert_eq!(
            ValueSource::from(&PidValueSource::BackupRow {
                table: "T_PIPINGPOINT",
                column: "PIPINGPOINTNUMBER",
            }),
            ValueSource::DatabaseColumn {
                table: "T_PIPINGPOINT".to_string(),
                column: "PIPINGPOINTNUMBER".to_string(),
            }
        );
    }
}
