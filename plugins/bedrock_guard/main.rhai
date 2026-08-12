// A protection plugin: refuses edits it doesn't like.
//
// This is why the block hooks are cancellable rather than
// notifications -- the answer has to arrive before the world changes,
// not as an undo afterwards.

// Rhai functions don't capture script-level constants -- they're
// isolated on purpose -- so shared values are written as functions.
fn spawn_radius() { 3 }

fn is_protected(x, y, z) {
    if y <= 1 { return true; }   // the world's floor
    let r = spawn_radius();
    if x * x + z * z <= r * r {  // spawn area
        return true;
    }
    return false;
}

fn on_block_break(player, x, y, z) {
    if is_protected(x, y, z) {
        tell(player, "That block is protected.");
        return false;
    }
    return true;
}

fn on_block_place(player, x, y, z, block) {
    if is_protected(x, y, z) {
        tell(player, "You can't build here.");
        return false;
    }
    return true;
}
