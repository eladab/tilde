//! Space themes: background tints and small pixel-art pieces.
//!
//! Art is drawn with half-block characters, two pixels per cell, so a piece
//! 14 pixels tall takes 7 rows of the sidebar.

/// Background tints, all dark and low in saturation so text stays readable
/// and the page remains the brightest thing in the window. Index 0 is the
/// original background.
pub const BACKGROUNDS: &[(&str, (u8, u8, u8))] = &[
    ("ink", (23, 23, 28)),
    ("graphite", (27, 27, 27)),
    ("midnight", (17, 22, 36)),
    ("ocean", (15, 28, 33)),
    ("forest", (19, 29, 24)),
    ("plum", (30, 22, 37)),
    ("wine", (36, 22, 27)),
    ("ember", (35, 26, 21)),
];

pub struct Art {
    pub name: &'static str,
    /// One string per pixel row; every row has the same length and there is
    /// an even number of rows. `.` is transparent.
    pub rows: &'static [&'static str],
    pub palette: &'static [(char, (u8, u8, u8))],
}

impl Art {
    pub fn width(&self) -> usize {
        self.rows.first().map(|r| r.chars().count()).unwrap_or(0)
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// Colour of one pixel, or `None` where the background shows through.
    pub fn pixel(&self, x: usize, y: usize) -> Option<(u8, u8, u8)> {
        let c = self.rows.get(y)?.chars().nth(x)?;
        self.palette.iter().find(|(key, _)| *key == c).map(|(_, rgb)| *rgb)
    }
}

/// Index 0 in a space means "no art"; piece `n` is `ART[n - 1]`.
pub const ART: &[Art] = &[
    Art {
        name: "mountains",
        rows: &[
            "........................",
            "....s..............mm...",
            "..................mmmm..",
            "..........s.......mmmm..",
            ".....w.............mm...",
            "....waa.......s.........",
            "...waaaa.....w..........",
            "..aaaaaaa...wbb.........",
            ".aaaaaaaaa.wbbbb....a...",
            "aaaaaaaaaabbbbbbb..aaa..",
            "aaaaaaaabbbbbbbbbbaaaaa.",
            "aaaaaaabbbbbbbbbbbbaaaaa",
            "gggggggggggggggggggggggg",
            "gggggggggggggggggggggggg",
        ],
        palette: &[
            ('m', (236, 226, 174)),
            ('s', (196, 202, 228)),
            ('w', (214, 220, 238)),
            ('a', (92, 102, 148)),
            ('b', (62, 70, 106)),
            ('g', (44, 50, 74)),
        ],
    },
    Art {
        name: "sunset",
        rows: &[
            "qqqqqqqqqqqqqqqqqqqqqqqq",
            "qqqqqqqqqqqqqqqqqqqqqqqq",
            "qqqqqqqqqqyyyyqqqqqqqqqq",
            "ppppppppyyyyyyyypppppppp",
            "pppppppyyyooooyyyppppppp",
            "ppppppyyooooooooyypppppp",
            "ppppppyooooooooooypppppp",
            "ppppppyooooooooooypppppp",
            "dddddddddrrrrrrddddddddd",
            "dlldddddddrrrrdddddllddd",
            "ddddddllddrrrrddlldddddd",
            "dddlldddddddrrdddddddlld",
            "dddddddddllddrdddddddddd",
            "dddddddddddddddddddddddd",
        ],
        palette: &[
            ('q', (74, 62, 112)),
            ('p', (140, 86, 118)),
            ('y', (250, 196, 128)),
            ('o', (246, 150, 96)),
            ('r', (226, 140, 104)),
            ('d', (40, 58, 98)),
            ('l', (72, 100, 150)),
        ],
    },
    Art {
        name: "cat",
        rows: &[
            "..c.........c...",
            "..cc.......cc...",
            "..ccccccccccc...",
            "..ccccccccccc...",
            "..ccecccccecc...",
            "..cccccnccccc...",
            "...ccwwwwwcc....",
            "....ccccccc.....",
            "...ccdcccdccc...",
            "...ccccccccccc..",
            "...cccccccccc..c",
            "...cwwcccwwc..cc",
            "...cwwcccwwcccc.",
            "................",
        ],
        palette: &[
            ('c', (214, 162, 108)),
            ('d', (166, 116, 76)),
            ('e', (124, 204, 150)),
            ('n', (238, 150, 156)),
            ('w', (238, 232, 220)),
        ],
    },
    Art {
        name: "plant",
        rows: &[
            "......g.......",
            ".....ggh......",
            "..g..ggh..g...",
            ".ggh.ggh.ggh..",
            ".gghhgghggh...",
            "..gghgghgh....",
            "...ggghhh.....",
            ".....gh.......",
            "..tttttttttt..",
            "..teeeeeeeet..",
            "...tttttttu...",
            "...tttttttu...",
            "....tttttu....",
            "....tttttu....",
        ],
        palette: &[
            ('g', (126, 196, 128)),
            ('h', (74, 146, 96)),
            ('t', (206, 126, 96)),
            ('u', (162, 94, 74)),
            ('e', (78, 58, 52)),
        ],
    },
    Art {
        name: "rocket",
        rows: &[
            "......rr......",
            ".s...rrrr.....",
            ".....wwwd.....",
            "....wwwwwd....",
            "....wbbwwd..s.",
            "....wbbwwd....",
            "....wwwwwd....",
            "...rwwwwwdr...",
            "..rrwwwwwdrr..",
            "..rr.wwwd.rr..",
            "s.....ff......",
            ".....fyyf.....",
            "......fy...s..",
            "......f.......",
        ],
        palette: &[
            ('w', (226, 230, 238)),
            ('d', (164, 170, 188)),
            ('r', (236, 112, 122)),
            ('b', (112, 170, 236)),
            ('f', (250, 162, 92)),
            ('y', (252, 220, 134)),
            ('s', (196, 202, 228)),
        ],
    },
];

/// The art a space shows, if any.
pub fn art(index: usize) -> Option<&'static Art> {
    index.checked_sub(1).and_then(|i| ART.get(i))
}

/// Display name for an art index, including "none".
pub fn art_name(index: usize) -> &'static str {
    art(index).map(|a| a.name).unwrap_or("none")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_is_a_clean_rectangle() {
        assert_eq!(ART.len(), 5);
        for piece in ART {
            assert!(piece.height() >= 2 && piece.height() % 2 == 0, "{}: two pixel rows per cell", piece.name);
            assert!(piece.width() <= 24, "{}: fits the sidebar", piece.name);
            for (y, row) in piece.rows.iter().enumerate() {
                assert_eq!(row.chars().count(), piece.width(), "{} row {y} has the wrong length", piece.name);
                for c in row.chars() {
                    assert!(c == '.' || piece.palette.iter().any(|(k, _)| *k == c), "{} row {y}: no colour for '{c}'", piece.name);
                }
            }
            // Every palette entry is used, and no key is defined twice.
            for (i, (key, _)) in piece.palette.iter().enumerate() {
                assert!(piece.rows.iter().any(|r| r.contains(*key)), "{}: '{key}' is never used", piece.name);
                assert!(piece.palette[..i].iter().all(|(k, _)| k != key), "{}: '{key}' defined twice", piece.name);
            }
        }
    }

    #[test]
    fn backgrounds_are_dark_enough_for_light_text() {
        assert_eq!(BACKGROUNDS[0].1, (23, 23, 28), "index 0 is the original background");
        for (name, (r, g, b)) in BACKGROUNDS {
            let luma = 0.2126 * *r as f64 + 0.7152 * *g as f64 + 0.0722 * *b as f64;
            assert!(luma < 32.0, "{name} is too bright ({luma:.1})");
            assert!(luma > 14.0, "{name} is too dark to tell from black ({luma:.1})");
        }
    }

    #[test]
    fn lookups() {
        assert!(art(0).is_none());
        assert_eq!(art(1).unwrap().name, "mountains");
        assert_eq!(art_name(0), "none");
        assert_eq!(art_name(5), "rocket");
        assert_eq!(art_name(99), "none");
        assert_eq!(art(3).unwrap().pixel(0, 0), None);
        assert_eq!(art(3).unwrap().pixel(2, 0), Some((214, 162, 108)));
    }

    /// `TILDE_ART_DUMP=dir cargo test dump_art` writes each piece as a text
    /// file of hex colours, for turning into images when designing.
    #[test]
    fn dump_art() {
        let Some(dir) = std::env::var_os("TILDE_ART_DUMP") else { return };
        for piece in ART {
            let mut out = String::new();
            for y in 0..piece.height() {
                let row: Vec<String> = (0..piece.width())
                    .map(|x| piece.pixel(x, y).map(|(r, g, b)| format!("{r:02x}{g:02x}{b:02x}")).unwrap_or_else(|| "------".into()))
                    .collect();
                out.push_str(&row.join(" "));
                out.push('\n');
            }
            std::fs::write(std::path::Path::new(&dir).join(format!("{}.txt", piece.name)), out).unwrap();
        }
    }
}
