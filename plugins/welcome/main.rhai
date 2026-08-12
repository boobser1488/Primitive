// Greets arrivals and adds a /players command.
//
// Shows the three basics: reacting to events, reading the world
// snapshot, and adding a command the server itself doesn't know about.

fn on_load() {
    log("welcome plugin ready");
}

fn on_join(player, username) {
    broadcast(username + " joined the server");
    tell(player, "Welcome to primitive! Type /online to see who's here.");
}

fn on_leave(player, username) {
    broadcast(username + " left");
}

// Returning false means "handled" -- the server then stays quiet
// instead of reporting an unknown command.
//
// Note the name: plugins only see commands the server doesn't already
// know. `/players` would never arrive here, because that's an alias of
// the built-in `/list`. Built-ins win, so a plugin can't shadow `/stop`.
fn on_command(player, command, args) {
    if command == "online" {
        let names = player_names();
        tell(player, "" + player_count() + " online: " + names);
        return false;
    }
    return true;
}
