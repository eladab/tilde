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
    ("y", "History, of every space"),
    ("d", "Downloads"),
    ("I", "Dev tools: network and storage"),
    ("?", "This key reference"),
    ("q", "Quit"),
];

/// The keys of the History view, shown along its bottom edge.
pub const HISTORY: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("⏎", "open in new tab"),
    ("s", "in its space"),
    ("x", "forget"),
    ("C", "clear all"),
    ("esc", "close"),
];

/// The keys of the Downloads view, shown along its bottom edge.
pub const DOWNLOADS: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("⏎", "open"),
    ("f", "show in Finder"),
    ("x", "stop or remove"),
    ("C", "clear list"),
    ("esc", "close"),
];

pub const ANYWHERE: &[(&str, &str)] = &[
    ("⌘.", "Switch between tabs and page"),
    ("⌘L", "Edit the address"),
    ("⌘C", "Copy the address while editing it"),
    ("⌥← / ⌥→", "Move by words while typing"),
    ("⌥⌫ / ⌘⌫", "Delete the word / the whole line while typing"),
    ("⌘T", "New tab"),
    ("⌘W", "Close tab"),
    ("⌘R", "Reload"),
    ("⌘[ / ⌘]", "Back / forward"),
    ("⌃Tab / ⌃⇧Tab", "Next / previous tab"),
    ("⌘1 … ⌘9", "Select tab (9 is the last)"),
    ("⌘D", "Pin or unpin tab"),
    ("⌘⇧H", "Return to the pinned page"),
    ("⌘B", "Show or hide the sidebar"),
    ("⌘+ / ⌘-", "Zoom the page in / out"),
    ("⌘0", "Actual size"),
    ("Pinch", "Zoom the page on the trackpad"),
    ("⌥⌘← / ⌥⌘→", "Previous / next space"),
    ("⌘⇧N", "New space"),
    ("⌃1 … ⌃9", "Select space"),
    ("⌘Y", "History"),
    ("⌥⌘L", "Downloads"),
    ("⌥⌘I", "Dev tools"),
    ("⌘/", "This key reference"),
    ("⌘-click", "Open link in a background tab"),
    ("⌘Q", "Quit"),
];

/// The keys of the dev tools pane, shown along its bottom edge.
pub const NET_LIST: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("⏎", "details"),
    ("f", "filter"),
    ("/", "search"),
    ("c", "copy"),
    ("C", "curl"),
    ("x", "clear"),
    ("p", "keep"),
    ("r", "reload"),
    ("tab", "storage"),
    ("m", "dock"),
    ("-/+", "size"),
    ("esc", "tabs"),
    ("q", "close"),
];

pub const NET_DETAIL: &[(&str, &str)] = &[
    ("j/k", "scroll"),
    ("h/l", "part"),
    ("J/K", "other request"),
    ("c", "copy this part"),
    ("C", "curl"),
    ("esc", "list"),
    ("q", "close"),
];

pub const STORAGE_LIST: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("h/l", "kind"),
    ("⏎", "details"),
    ("s", "site"),
    ("/", "search"),
    ("c", "copy"),
    ("x", "delete"),
    ("X", "all"),
    ("r", "refresh"),
    ("tab", "network"),
    ("m", "dock"),
    ("-/+", "size"),
    ("esc", "tabs"),
    ("q", "close"),
];

pub const STORAGE_DETAIL: &[(&str, &str)] = &[
    ("j/k", "scroll"),
    ("J/K", "other item"),
    ("c", "copy value"),
    ("x", "delete"),
    ("esc", "list"),
    ("q", "close"),
];

pub const STORAGE_SITES: &[(&str, &str)] = &[
    ("j/k", "move"),
    ("⏎", "show its storage"),
    ("/", "search"),
    ("X", "delete all its data"),
    ("r", "refresh"),
    ("esc", "back"),
];

pub const DEV_SEARCH: &[(&str, &str)] = &[("⏎", "keep"), ("esc", "clear")];
