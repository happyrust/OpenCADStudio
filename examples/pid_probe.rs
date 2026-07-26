//! Temporary probe: what does the `.pid` importer actually hand the scene?

use OpenCADStudio::io;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::PathBuf::from(&arg);
        let doc = match io::load_file(&path) {
            Ok(doc) => doc,
            Err(error) => {
                println!("{arg}: FAILED {error}");
                continue;
            }
        };
        println!("{arg}");
        println!("  entities   = {}", doc.entity_count());
        println!(
            "  ext_min    = ({:.2}, {:.2})  ext_max = ({:.2}, {:.2})",
            doc.header.model_space_extents_min.x,
            doc.header.model_space_extents_min.y,
            doc.header.model_space_extents_max.x,
            doc.header.model_space_extents_max.y
        );
        println!("  ms_block   = {:?}", doc.header.model_space_block_handle);
        let vports: Vec<_> = doc.vports.iter().collect();
        println!("  vports     = {}", vports.len());
        for vp in vports {
            println!(
                "    name={:?} handle={:?} view_height={:.3} target=({:.2}, {:.2}) center=({:.2}, {:.2}) dir=({:.1},{:.1},{:.1}) ll=({:.2},{:.2}) ur=({:.2},{:.2})",
                vp.name, vp.handle, vp.view_height,
                vp.view_target.x, vp.view_target.y,
                vp.view_center.x, vp.view_center.y,
                vp.view_direction.x, vp.view_direction.y, vp.view_direction.z,
                vp.lower_left.x, vp.lower_left.y,
                vp.upper_right.x, vp.upper_right.y
            );
        }
        let mut owned = 0usize;
        for e in doc.entities() {
            if e.common().owner_handle == doc.header.model_space_block_handle {
                owned += 1;
            }
        }
        println!("  owned_by_model_space = {owned}");
    }
}