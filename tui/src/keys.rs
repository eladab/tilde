//! The key reference shown by `?`. Keep it in step with `App::handle_nav_key`
//! and with the app's main menu.

pub const NAV: &[(&str, &str)] = &[
    ("j / k", "Next / previous tab"),
    ("Enter, i", "Go to the page"),
    ("o", "Edit the address"),
    ("t", "New tab"),
    ("x", "Close tab (a pinned tab is unloaded)"),
    ("H / L", "Back / forward"),
    ("r", "Reload, or stop while loading"),
    ("J / K", "Move tab down / up"),
    ("p", "Pin or unpin tab"),
    ("P", "Return to the pinned page"),
    ("n", "Rename tab (empty restores the title)"),
    ("b", "Show or hide the sidebar"),
    ("h / l", "Previous / next space"),
    ("N", "New space"),
    ("e", "Edit space: name, colours, art, profile"),
    ("D", "Delete space"),
    ("?", "This key reference"),
    ("q", "Quit"),
];

pub const ANYWHERE: &[(&str, &str)] = &[
    ("⌘.", "Switch between tabs and page"),
    ("⌘L", "Edit the address"),
    ("⌘T", "New tab"),
    ("⌘W", "Close tab"),
    ("⌘R", "Reload"),
    ("⌘[ / ⌘]", "Back / forward"),
    ("⌃Tab / ⌃⇧Tab", "Next / previous tab"),
    ("⌘1 … ⌘9", "Select tab (9 is the last)"),
    ("⌘D", "Pin or unpin tab"),
    ("⌘⇧H", "Return to the pinned page"),
    ("⌘B", "Show or hide the sidebar"),
    ("⌥⌘← / ⌥⌘→", "Previous / next space"),
    ("⌘⇧N", "New space"),
    ("⌃1 … ⌃9", "Select space"),
    ("⌘/", "This key reference"),
    ("⌘-click", "Open link in a background tab"),
    ("⌘Q", "Quit"),
];
