//! Prints every property of every block, one line each.
//!
//! A golden dump, for changing *how* the properties are stored without
//! changing what any of them is. Run it before a refactor and after it,
//! and diff: anything that moved is a mistake, because moving them was
//! not the point.
//!
//! ```text
//! cargo run -p primitive_shared --example dump_block_properties > before.txt
//! ```

use primitive_shared::types::*;

fn main() {
    for &(id, name) in ALL_BLOCK_IDS {
        println!(
            "{name}\tid={id}\tcross={}\tflat={}\titem={}\tcontainer={}\tloose={}\t\
             liquid={}\tfoliage={}\torientable={}\tgravity={}\tdisplaceable={}\t\
             collidable={}\topaque={}\ttranslucent={}\tcutout={}\ttargetable={}\t\
             placeable={}\tknown={}\tsupport={}\topacity={}\temission={}\t\
             break={:?}\tbreakable={}\tweight={}\tdrop={:?}\tdrops={}\t\
             inset={}\tturns={}\tdrag={}\theight={}\tcollision={}\tfull_top={}",
            is_cross(id),
            is_flat(id),
            is_item(id),
            is_container(id),
            is_loose(id),
            is_liquid(id),
            is_foliage(id),
            is_orientable(id),
            is_affected_by_gravity(id),
            can_be_displaced_by_falling(id),
            is_collidable(id),
            is_opaque(id),
            is_translucent(id),
            is_cutout(id),
            is_targetable(id),
            is_placeable(id),
            is_known_block(id),
            needs_support(id),
            light_opacity(id),
            light_emission(id),
            break_seconds(id),
            is_breakable(id),
            block_weight(id),
            block_drop(id),
            block_drop_count(id),
            flat_inset(id),
            texture_turns(id, 0),
            surface_drag(id),
            block_height(id),
            collision_height(id),
            has_full_top(id),
        );
    }

    // Air is not in the table and still has to answer for itself.
    println!(
        "air\tcollidable={}\topaque={}\tknown={}\tair={}",
        is_collidable(BLOCK_AIR),
        is_opaque(BLOCK_AIR),
        is_known_block(BLOCK_AIR),
        is_air(BLOCK_AIR),
    );

    // What grows on what, which is a pair rather than a property.
    for &(plant, plant_name) in ALL_BLOCK_IDS {
        for &(ground, ground_name) in ALL_BLOCK_IDS {
            if can_grow_on(plant, ground) {
                println!("grows\t{plant_name}\ton\t{ground_name}");
            }
        }
    }
}
