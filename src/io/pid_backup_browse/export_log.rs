//! The tables of a backup's database, and how many rows each holds.
//!
//! `Export.dmp` is the plant's database: four schemas, some 150 tables. What
//! is in it is written down twice beside it. `Manifest.txt` names every
//! table (`Table<<|>>schema<<|>>name`), and `Export.log` -- what Oracle's
//! `exp` printed while it wrote the dump -- names them again with the number
//! of rows it exported:
//!
//! ```text
//! . about to export SQPLANTPID's tables via Conventional Path ...
//! . . exporting table                      T_DRAWING         53 rows exported
//! ```
//!
//! This module reads the log and merges the two into a [`TableCatalog`]: the
//! schemas in the manifest's order, each table with its row count when the
//! log has one. A backup with no log (a SQL Server one writes none) lists
//! the manifest's tables without counts; one with no manifest lists the
//! log's. Reading the rows themselves is not this module's business.

use super::labels::natural_cmp;
use super::manifest::DbObject;

/// What an `Export.log` says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportLog {
    /// The database that wrote the dump (`Connected to: …`).
    pub server: Option<String>,
    /// The character sets of the export (`Export done in …`).
    pub character_sets: Option<String>,
    /// How the export ended (`Export terminated …`).
    pub outcome: Option<String>,
    /// The `EXP-` and `ORA-` messages, in the order they were printed.
    pub messages: Vec<String>,
    /// The schemas in the order they were exported, with their tables.
    pub schemas: Vec<LoggedSchema>,
}

/// One schema of an [`ExportLog`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoggedSchema {
    /// The schema's name, as the log spells it (upper case).
    pub name: String,
    /// Its tables in the order they were exported: the name, and the rows
    /// exported when the log states them.
    pub tables: Vec<(String, Option<u64>)>,
}

/// Read the lines of an `Export.log`.
pub fn parse_export_log<'a>(lines: impl IntoIterator<Item = &'a str>) -> ExportLog {
    let mut log = ExportLog::default();
    for line in lines {
        let text = line.trim();
        if let Some(server) = text.strip_prefix("Connected to:") {
            log.server.get_or_insert_with(|| server.trim().to_string());
        } else if let Some(sets) = text.strip_prefix("Export done in ") {
            log.character_sets
                .get_or_insert_with(|| sets.trim().to_string());
        } else if text.starts_with("Export terminated") {
            log.outcome = Some(text.to_string());
        } else if text.starts_with("EXP-") || text.starts_with("ORA-") {
            log.messages.push(text.to_string());
        } else if let Some(schema) = schema_of(text) {
            log.schemas.push(LoggedSchema {
                name: schema.to_string(),
                tables: Vec::new(),
            });
        } else if let Some(rest) = after(text, "exporting table") {
            let mut words = rest.split_whitespace();
            let (Some(name), Some(schema)) = (words.next(), log.schemas.last_mut()) else {
                continue;
            };
            schema.tables.push((name.to_string(), rows_exported(words)));
        } else if let Some(rest) = after(text, "exporting partition")
            .or_else(|| after(text, "exporting subpartition"))
            .or_else(|| after(text, "exporting composite partition"))
        {
            // A partitioned table's rows are stated partition by partition.
            let mut words = rest.split_whitespace();
            let _name = words.next();
            let (Some(rows), Some(table)) = (
                rows_exported(words),
                log.schemas
                    .last_mut()
                    .and_then(|schema| schema.tables.last_mut()),
            ) else {
                continue;
            };
            table.1 = Some(table.1.unwrap_or(0) + rows);
        }
    }
    log
}

/// What follows `marker` in a line of dots and words (`. . exporting table
/// T_DRAWING …`). The marker ends at a space: `exporting tablespace
/// definitions` exports no table.
fn after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = text.trim_start_matches(['.', ' ']);
    rest.strip_prefix(marker)
        .filter(|rest| rest.starts_with(char::is_whitespace))
}

/// The schema of an `about to export SCHEMA's tables` line.
fn schema_of(text: &str) -> Option<&str> {
    let rest = text.trim_start_matches(['.', ' ']);
    let rest = rest
        .strip_prefix("about to export ")
        .or_else(|| rest.strip_prefix("About to export "))?;
    let (schema, what) = rest.split_once("'s ")?;
    (what.starts_with("tables") && !schema.is_empty() && !schema.contains(' ')).then_some(schema)
}

/// `53` of `53 rows exported`.
fn rows_exported<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<u64> {
    let rows = words.next()?.parse().ok()?;
    (words.next() == Some("rows") && words.next() == Some("exported")).then_some(rows)
}

/// The tables of a backup's database.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableCatalog {
    /// The schemas: the manifest's, in its order, then any only the log has.
    pub schemas: Vec<CatalogSchema>,
    /// Whether an export log was read: without one no table has a count.
    pub has_log: bool,
    /// The database that wrote the dump, from the log.
    pub server: Option<String>,
    /// The character sets of the export, from the log.
    pub character_sets: Option<String>,
    /// How the export ended, from the log.
    pub outcome: Option<String>,
    /// The `EXP-` and `ORA-` messages of the log.
    pub messages: Vec<String>,
    /// The schemas in the order the export wrote them, which is the order
    /// their tables lie in the dump. Empty without a log.
    pub export_order: Vec<String>,
}

/// One schema of a [`TableCatalog`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogSchema {
    /// The schema's name, as the manifest spells it when it names it.
    pub name: String,
    /// Its tables, by name.
    pub tables: Vec<CatalogTable>,
}

/// One table of a [`CatalogSchema`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogTable {
    /// The table's name, as the manifest spells it when it names it.
    pub name: String,
    /// The rows the export wrote, when the log states them.
    pub rows: Option<u64>,
    /// Whether `Manifest.txt` names the table.
    pub in_manifest: bool,
    /// Whether `Export.log` names the table.
    pub in_log: bool,
}

impl CatalogSchema {
    /// The rows of all its tables; `None` when no table has a count.
    pub fn rows(&self) -> Option<u64> {
        self.tables
            .iter()
            .filter_map(|table| table.rows)
            .reduce(|sum, rows| sum + rows)
    }

    /// How many of its tables hold at least one row.
    pub fn filled(&self) -> usize {
        self.tables
            .iter()
            .filter(|table| table.rows.is_some_and(|rows| rows > 0))
            .count()
    }
}

impl TableCatalog {
    /// How many tables the schemas hold together.
    pub fn table_count(&self) -> usize {
        self.schemas.iter().map(|schema| schema.tables.len()).sum()
    }

    /// The schema whose table `table` comes first in the dump: the one a
    /// reader that takes "the first table of that name" reads. That is the
    /// export's order when the log gives it, the catalog's otherwise.
    pub fn first_schema_with(&self, table: &str) -> Option<usize> {
        let has = |schema: &CatalogSchema| {
            schema
                .tables
                .iter()
                .any(|found| found.name.eq_ignore_ascii_case(table))
        };
        self.export_order
            .iter()
            .filter_map(|name| {
                self.schemas
                    .iter()
                    .position(|schema| schema.name.eq_ignore_ascii_case(name))
            })
            .find(|at| has(&self.schemas[*at]))
            .or_else(|| self.schemas.iter().position(has))
    }

    /// The rows of all the tables; `None` when no table has a count.
    pub fn rows(&self) -> Option<u64> {
        self.schemas
            .iter()
            .filter_map(CatalogSchema::rows)
            .reduce(|sum, rows| sum + rows)
    }

    /// How many tables only one of the two sources names: `(manifest only,
    /// log only)`. Both zero when there is no log to compare with.
    pub fn one_sided(&self) -> (usize, usize) {
        if !self.has_log {
            return (0, 0);
        }
        let count = |keep: fn(&CatalogTable) -> bool| {
            self.schemas
                .iter()
                .flat_map(|schema| &schema.tables)
                .filter(|table| keep(table))
                .count()
        };
        (
            count(|table| table.in_manifest && !table.in_log),
            count(|table| !table.in_manifest && table.in_log),
        )
    }
}

/// Merge the manifest's tables with what the export log says of them.
/// Schemas and tables are matched without regard to case: the manifest
/// writes `SQPlantpid`, the log `SQPLANTPID`. `None` when neither names a
/// table.
pub fn table_catalog(manifest: &[DbObject], log: Option<&ExportLog>) -> Option<TableCatalog> {
    let mut schemas: Vec<CatalogSchema> = Vec::new();
    let schema_at = |schemas: &mut Vec<CatalogSchema>, name: &str| -> usize {
        match schemas
            .iter()
            .position(|schema| schema.name.eq_ignore_ascii_case(name))
        {
            Some(at) => at,
            None => {
                schemas.push(CatalogSchema {
                    name: name.to_string(),
                    tables: Vec::new(),
                });
                schemas.len() - 1
            }
        }
    };
    for object in manifest {
        let at = schema_at(&mut schemas, &object.schema);
        let tables = &mut schemas[at].tables;
        if !tables
            .iter()
            .any(|table| table.name.eq_ignore_ascii_case(&object.name))
        {
            tables.push(CatalogTable {
                name: object.name.clone(),
                rows: None,
                in_manifest: true,
                in_log: false,
            });
        }
    }
    for logged in log.into_iter().flat_map(|log| &log.schemas) {
        let at = schema_at(&mut schemas, &logged.name);
        for (name, rows) in &logged.tables {
            let tables = &mut schemas[at].tables;
            match tables
                .iter_mut()
                .find(|table| table.name.eq_ignore_ascii_case(name))
            {
                Some(table) => {
                    table.in_log = true;
                    // A table exported in two runs of one log counts both.
                    table.rows = match (table.rows, *rows) {
                        (Some(known), Some(more)) => Some(known + more),
                        (known, more) => known.or(more),
                    };
                }
                None => tables.push(CatalogTable {
                    name: name.clone(),
                    rows: *rows,
                    in_manifest: false,
                    in_log: true,
                }),
            }
        }
    }
    for schema in &mut schemas {
        schema
            .tables
            .sort_by(|left, right| natural_cmp(&left.name, &right.name));
    }
    schemas.retain(|schema| !schema.tables.is_empty());
    if schemas.is_empty() {
        return None;
    }
    Some(TableCatalog {
        schemas,
        has_log: log.is_some(),
        server: log.and_then(|log| log.server.clone()),
        character_sets: log.and_then(|log| log.character_sets.clone()),
        outcome: log.and_then(|log| log.outcome.clone()),
        messages: log.map(|log| log.messages.clone()).unwrap_or_default(),
        export_order: log
            .map(|log| {
                log.schemas
                    .iter()
                    .map(|schema| schema.name.clone())
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a plant's export log, cut down to two schemas.
    const LOG: &str = "
Connected to: Oracle Database 12c Enterprise Edition Release 12.1.0.2.0 - 64bit Production
With the Partitioning, OLAP, Advanced Analytics and Real Application Testing options
Export done in AL32UTF8 character set and AL16UTF16 NCHAR character set

About to export specified users ...
. exporting pre-schema procedural objects and actions
. exporting foreign function library names for user SQPLANTD 
About to export SQPLANTD's objects ...
. exporting database links
. about to export SQPLANTD's tables via Conventional Path ...
. . exporting table                   APPLICATIONS          5 rows exported
. . exporting table                      CODELISTS        130 rows exported
. . exporting table                     INTERSECTS          0 rows exported
. exporting tablespace definitions
. exporting synonyms
About to export SQPLANTPID's objects ...
. about to export SQPLANTPID's tables via Conventional Path ...
. . exporting table                      T_DRAWING         53 rows exported
. . exporting table                    T_MODELITEM
. . exporting partition                         P_2023      40000 rows exported
. . exporting partition                         P_2024       7124 rows exported
EXP-00091: Exporting questionable statistics.
. . exporting table                         T_NOTE
EXP-00008: ORACLE error 942 encountered
ORA-00942: table or view does not exist
. exporting statistics
Export terminated successfully with warnings.
";

    fn object(schema: &str, name: &str) -> DbObject {
        DbObject {
            schema: schema.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn the_log_names_its_schemas_and_the_rows_of_every_table() {
        let log = parse_export_log(LOG.lines());
        assert_eq!(
            log.server.as_deref(),
            Some("Oracle Database 12c Enterprise Edition Release 12.1.0.2.0 - 64bit Production")
        );
        assert_eq!(
            log.character_sets.as_deref(),
            Some("AL32UTF8 character set and AL16UTF16 NCHAR character set")
        );
        assert_eq!(
            log.outcome.as_deref(),
            Some("Export terminated successfully with warnings.")
        );
        assert_eq!(
            log.messages,
            [
                "EXP-00091: Exporting questionable statistics.",
                "EXP-00008: ORACLE error 942 encountered",
                "ORA-00942: table or view does not exist",
            ]
        );
        // "About to export SQPLANTD's objects" opens no schema: the tables
        // line does.
        assert_eq!(
            log.schemas,
            [
                LoggedSchema {
                    name: "SQPLANTD".to_string(),
                    tables: vec![
                        ("APPLICATIONS".to_string(), Some(5)),
                        ("CODELISTS".to_string(), Some(130)),
                        ("INTERSECTS".to_string(), Some(0)),
                    ],
                },
                LoggedSchema {
                    name: "SQPLANTPID".to_string(),
                    tables: vec![
                        ("T_DRAWING".to_string(), Some(53)),
                        // A partitioned table is the sum of its partitions.
                        ("T_MODELITEM".to_string(), Some(47_124)),
                        // A table whose export failed has no count.
                        ("T_NOTE".to_string(), None),
                    ],
                },
            ]
        );
    }

    #[test]
    fn a_log_that_is_not_an_export_log_names_nothing() {
        assert_eq!(parse_export_log([]), ExportLog::default());
        let log = parse_export_log(
            "some other log\n. . exporting table ORPHAN 3 rows exported\nabout to export nothing\n"
                .lines(),
        );
        // A table with no schema above it is not filed anywhere.
        assert!(log.schemas.is_empty() && log.outcome.is_none());
    }

    #[test]
    fn the_catalog_is_the_manifests_tables_with_the_logs_row_counts() {
        let manifest = [
            object("SQPlantpid", "T_Note"),
            object("SQPlantpid", "T_Drawing"),
            object("SQPlantpid", "T_ModelItem"),
            object("SQPlantd", "CodeLists"),
            object("SQPlantd", "Applications"),
            object("SQPlantd", "Intersects"),
            // Only the manifest knows this one.
            object("SQPlantd", "Version"),
        ];
        let mut log = parse_export_log(LOG.lines());
        // Only the log knows this one, and a schema of its own.
        log.schemas[0].tables.push(("EXTRA".to_string(), Some(7)));
        log.schemas.push(LoggedSchema {
            name: "AUDIT".to_string(),
            tables: vec![("TRAIL".to_string(), Some(2))],
        });
        let catalog = table_catalog(&manifest, Some(&log)).expect("there are tables");
        assert!(catalog.has_log);
        // The manifest's order and spelling of the schemas; the log's last.
        let shape: Vec<(&str, Vec<(&str, Option<u64>)>)> = catalog
            .schemas
            .iter()
            .map(|schema| {
                (
                    schema.name.as_str(),
                    schema
                        .tables
                        .iter()
                        .map(|table| (table.name.as_str(), table.rows))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                (
                    "SQPlantpid",
                    vec![
                        ("T_Drawing", Some(53)),
                        ("T_ModelItem", Some(47_124)),
                        ("T_Note", None),
                    ]
                ),
                (
                    "SQPlantd",
                    vec![
                        ("Applications", Some(5)),
                        ("CodeLists", Some(130)),
                        ("EXTRA", Some(7)),
                        ("Intersects", Some(0)),
                        ("Version", None),
                    ]
                ),
                ("AUDIT", vec![("TRAIL", Some(2))]),
            ]
        );
        assert_eq!(catalog.table_count(), 9);
        assert_eq!(catalog.rows(), Some(53 + 47_124 + 5 + 130 + 7 + 2));
        assert_eq!(catalog.schemas[1].rows(), Some(142));
        // `Intersects` is exported and empty; `Version` was not counted.
        assert_eq!(catalog.schemas[1].filled(), 3);
        assert_eq!(catalog.one_sided(), (1, 2));
        assert_eq!(
            catalog.outcome.as_deref(),
            Some("Export terminated successfully with warnings.")
        );
        assert_eq!(catalog.messages.len(), 3);
    }

    #[test]
    fn a_table_two_schemas_have_is_first_in_the_one_the_export_wrote_first() {
        // The manifest names SQPlantd before SQPlantpidd; the export wrote
        // them the other way round, and that is how they lie in the dump.
        let manifest = [
            object("SQPlantd", "CODELISTS"),
            object("SQPlantpidd", "CODELISTS"),
            object("SQPlantpid", "T_PIPERUN"),
        ];
        let log = parse_export_log(
            "
. about to export SQPLANTPIDD's tables via Conventional Path ...
. . exporting table                      CODELISTS       3407 rows exported
. about to export SQPLANTD's tables via Conventional Path ...
. . exporting table                      CODELISTS        130 rows exported
. about to export SQPLANTPID's tables via Conventional Path ...
. . exporting table                    T_MODELITEM      47124 rows exported
. . exporting table                      T_PIPERUN       4686 rows exported
"
            .lines(),
        );
        let catalog = table_catalog(&manifest, Some(&log)).unwrap();
        assert_eq!(catalog.schemas[0].name, "SQPlantd");
        assert_eq!(
            catalog.export_order,
            ["SQPLANTPIDD", "SQPLANTD", "SQPLANTPID"]
        );
        assert_eq!(catalog.first_schema_with("codelists"), Some(1));
        assert_eq!(catalog.first_schema_with("T_PIPERUN"), Some(2));
        assert_eq!(catalog.first_schema_with("T_NOZZLE"), None);
        // Without a log the catalog's own order is all there is.
        let catalog = table_catalog(&manifest, None).unwrap();
        assert!(catalog.export_order.is_empty());
        assert_eq!(catalog.first_schema_with("CODELISTS"), Some(0));
    }

    #[test]
    fn a_backup_without_a_log_lists_its_tables_without_counts() {
        let manifest = [object("TEST02", "T_Drawing"), object("TEST02", "MAX_ID")];
        let catalog = table_catalog(&manifest, None).expect("there are tables");
        assert!(!catalog.has_log);
        assert_eq!(catalog.table_count(), 2);
        assert_eq!(catalog.rows(), None);
        assert_eq!(catalog.schemas[0].rows(), None);
        assert_eq!(catalog.schemas[0].filled(), 0);
        // Nothing to compare the manifest with.
        assert_eq!(catalog.one_sided(), (0, 0));
        assert_eq!(
            catalog.schemas[0]
                .tables
                .iter()
                .map(|table| table.name.as_str())
                .collect::<Vec<_>>(),
            ["MAX_ID", "T_Drawing"]
        );
    }

    #[test]
    fn a_backup_that_names_no_table_has_no_catalog() {
        assert_eq!(table_catalog(&[], None), None);
        assert_eq!(table_catalog(&[], Some(&ExportLog::default())), None);
        // A log alone is enough.
        let log = parse_export_log(LOG.lines());
        let catalog = table_catalog(&[], Some(&log)).expect("the log names tables");
        assert_eq!(catalog.table_count(), 6);
        assert_eq!(catalog.one_sided(), (0, 6));
    }
}
