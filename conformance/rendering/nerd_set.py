"""What the `nerd` set says, symbol by symbol: the icon names that would
say it, most fitting first, and which symbols are marks rather than icons.

Data only — `generate_nerd.py` chooses among the candidates by
measurement, and `metrics.py` exempts the marks from *One size*, so both
read it from here.
"""

# Symbol -> candidate icon names, most fitting first. Order matters twice:
# it is the file's order, and within a list it is the order of meaning.
ICONS: dict[str, list[str]] = {
    "mark.native": ["cod-pass", "cod-check"],
    "mark.official": ["cod-verified_filled", "cod-verified"],
    "mark.ok": ["cod-pass", "cod-check"],
    "mark.adapted": ["cod-arrow_swap", "md-swap_horizontal"],
    "mark.unsupported": ["cod-circle_slash", "md-cancel"],
    "mark.attention": ["cod-warning", "md-alert"],
    "mark.close": ["cod-close", "md-close"],
    "mark.failed": ["cod-error", "md-close_circle"],
    "mark.dot": ["md-circle_small", "cod-circle_small_filled"],
    "mark.done": ["cod-pass", "cod-check"],
    "mark.sparkle": ["cod-sparkle_filled", "cod-sparkle", "oct-sparkle_fill"],
    "mark.toggle-off": ["cod-circle", "cod-circle_large"],
    "mark.toggle-on": ["cod-record", "cod-circle_large_filled"],
    "task.ready": ["cod-git_pull_request_create", "cod-git_pull_request"],
    "task.retry": ["cod-debug_rerun", "cod-refresh"],
    "status.completed": ["cod-pass", "cod-check"],
    "status.selected": ["cod-circle_filled"],
    "status.idle": ["cod-circle"],
    "arrow.up": ["md-arrow_up", "cod-arrow_up"],
    "arrow.down": ["md-arrow_down", "cod-arrow_down"],
    "arrow.external": ["cod-link_external", "md-open_in_new"],
    "arrow.swap": ["cod-arrow_swap", "md-swap_horizontal"],
    "arrow.to": ["md-arrow_right", "cod-arrow_right"],
    "chevron.right": ["cod-chevron_right"],
    "chevron.collapsed": ["fa-chevron_right", "cod-chevron_right"],
    "chevron.expanded": ["fa-chevron_down", "cod-chevron_down"],
    "prompt": ["cod-chevron_right"],
    "menu": ["cod-ellipsis"],
    "manage": ["fa-bars", "cod-menu"],
    "code": ["cod-code", "cod-symbol_namespace", "cod-file_code"],
    "architect": ["cod-type_hierarchy", "cod-symbol_structure", "md-sitemap"],
    "map": ["md-view_grid", "cod-layout"],
    "changes": ["cod-diff", "cod-git_compare"],
    "file.directory": ["cod-folder", "md-folder"],
    "file.directory-open": ["cod-folder_opened", "md-folder_open"],
    "file.default": ["cod-file", "md-file"],
    "file.code": ["cod-file_code", "md-file_code"],
    "file.markup": ["cod-markdown", "md-file_document", "cod-book"],
    "file.config": ["cod-settings_gear", "md-cog"],
    "file.lock": ["cod-lock", "md-lock"],
    "file.data": ["cod-database", "md-database", "fa-database"],
    "file.image": ["cod-file_media", "md-file_image"],
    "file.archive": ["cod-file_zip", "cod-archive", "md-zip_box"],
    "file.git": ["md-git", "cod-source_control"],
    "file.legal": ["cod-law", "md-scale_balance"],
}

# Symbols drawn at text weight by meaning, and why. Exempt from One size.
MARKS: dict[str, str] = {
    "mark.dot": "a bullet in running text is the size of a letter's dot",
    "mark.close": "a close cross sits in a control's corner at text weight",
    "mark.toggle-off": "a radio mark is drawn at the size of the text it selects",
    "mark.toggle-on": "a radio mark is drawn at the size of the text it selects",
    "status.selected": "a status dot marks a tab without competing with its name",
    "status.idle": "a status dot marks a tab without competing with its name",
    "arrow.up": "a direction arrow sits beside a number, at the number's size",
    "arrow.down": "a direction arrow sits beside a number, at the number's size",
    "arrow.external": "an external-link arrow trails a label, at its size",
    "arrow.swap": "a mapping arrow sits between two words, at their size",
    "arrow.to": "a mapping arrow sits between two words, at their size",
    "chevron.right": "a caret points at a line of text, at its size",
    "chevron.collapsed": "a disclosure caret leads a heading, at its size",
    "chevron.expanded": "a disclosure caret leads a heading, at its size",
    "prompt": "a prompt caret leads an input, at its size",
    "menu": "an ellipsis is typography: three dots at text height",
}

# Candidates the Rendering Lab measured out, and what it saw. A font's
# outlines are not the whole story: WezTerm and Ghostty redraw the icons of
# a Symbols-only fallback at a size of their own choosing, per icon, and
# these came out outside the band there while passing everywhere else.
# The generator skips them; the evidence is in `evidence/catalog.json`'s
# history.
LAB_REJECTED: dict[str, dict[str, str]] = {
    "mark.sparkle": {
        "cod-sparkle_filled": "WezTerm draws it 25% under the set, Ghostty 38% over, "
        "as a Symbols-only fallback",
        "cod-sparkle": "WezTerm draws it 25% under the set as a Symbols-only fallback, "
        "at the font's own size while it rescales every other icon",
    },
    "file.config": {
        "cod-settings_gear": "Ghostty draws it 20% over the set as a Symbols-only fallback",
    },
}
