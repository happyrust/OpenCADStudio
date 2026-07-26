//! Temporary probe: re-check the R2010+ ACAD_TABLE parse under the current
//! acadrust pin. See goals/perf-open-baseline/acadrust-r2010-table-bug.md.

fn main() {
    for path in std::env::args().skip(1) {
        let started = std::time::Instant::now();
        let doc = acadrust::io::dwg::DwgReader::from_file(&path)
            .unwrap()
            .read()
            .unwrap();
        println!("{path} -> {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
        for e in doc.entities() {
            if let acadrust::EntityType::Table(t) = e {
                let cells: usize = t.rows.iter().map(|r| r.cells.len()).sum();
                println!(
                    "  TABLE {:#X} rows={} columns={} cells={}",
                    t.common.handle,
                    t.rows.len(),
                    t.columns.len(),
                    cells
                );
            }
        }
    }
}
