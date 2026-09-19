## MODIFIED Requirements

### Requirement: Nothing an extension draws can name a colour or a glyph outside the vocabulary

An extension SHALL continue to describe its content in semantic terms only.
Introducing the theme vocabulary SHALL NOT give an extension a way to name a
chrome colour, and SHALL NOT require an extension to know the theme schema.

A glyph that *is* the content — a diagram's lines, corners, junctions and
arrowheads, a rendered document's rule — SHALL be allowed through as given,
the way a colour that belongs to content is. Such a glyph SHALL be coloured
by a semantic role and never by a value, SHALL be written by a file named
for it with the reason stated, and SHALL have a plain ASCII form wherever
the drawing depends on it to be read. A glyph that marks, separates or
labels the surface around the content remains chrome and SHALL come from
the vocabulary.

#### Scenario: An extension's chrome follows the active theme

- **WHEN** an extension's view is drawn under a non-default theme
- **THEN** its chrome is drawn from that theme, resolved by the host, with
  no change to what the extension itself produced

#### Scenario: Content with its own colours is still allowed through

- **WHEN** an extension supplies a colour for content that carries its own
  palette
- **THEN** that colour is drawn as given, and it remains the only way an
  extension can put a specific colour on screen

#### Scenario: A diagram's lines are content

- **WHEN** an extension draws a diagram whose boxes and edges are made of
  box-drawing characters
- **THEN** those characters are drawn as given, in the colour the active
  theme resolves for the role each carries

#### Scenario: A diagram drawn where box-drawing cannot be trusted

- **WHEN** the operator switches the diagram to its ASCII rendering
- **THEN** every glyph the drawing depends on is an ASCII character

#### Scenario: A chrome glyph written inline by an extension

- **WHEN** a file that is not named for content glyphs writes a chrome
  glyph inline
- **THEN** the build fails, naming the file and the rule
