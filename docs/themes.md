# Local VS Code code themes

Open Settings → Appearance → Colors. Light code theme and Dark code theme have
independent selectors. The folder button imports a local VS Code JSON/JSONC
color-theme file. The info button shows the selected IDs, linked source paths
and coverage/reload diagnostics; Copy details copies the complete report.
The commands Select code theme…, Import VS Code theme… and Inspect code theme…
open these same Settings controls and work without a repository.

Selection previews across all application windows. Save writes the selection to
TOML; Discard restores the saved choice. Importing itself installs a small catalog
link immediately, so an imported theme remains available after discarding a
selection. Custom shell seeds, fonts, comparison state, and update consent are
not changed. Shell color source → From code theme derives the shell around the
selected code background; Custom colors keeps its existing seed recipe.

```toml
[theme.code]
light = "builtin:light"
dark = "vscode:0123456789abcdef0123456789abcdef" # use the ID from inspection
```

IDs are namespaced hashes of the explicitly selected canonical file path, not
display names. Reimporting the same path updates its registration; same-name
files remain separate and receive short ID suffixes in menus. Renaming a source
file requires reimporting and selecting it again. Catalog links live alongside
the configuration in `themes/code/*.json`. Source files and their includes are
read-only. No extensions, marketplace, grammar packages or language servers are
installed or executed; no network requests are made.

Selected files and their dependencies are content-checked every two seconds on
a background worker. Unchanged files reuse the normalized asset. Source edits,
including atomic file replacement, reload without resetting editor entities or
font settings. Missing or malformed reloads retain the last valid colors for
that exact theme ID and appearance during the run; restarting without a valid
source uses built-in colors and reports the failure. Old async loads cannot
replace a newer selection or Discard. Choose built-in to recover immediately.
Inspect reports coverage limitations even when loading succeeds.

## Supported input and limits

JSONC comments, trailing commas, `colors`, `tokenColors`, local `include`,
comma-separated selectors, selector arrays, inherited rules, bold and italic are
supported. Explicit empty `fontStyle` clears inherited bold/italic. Foreground
and font style inherit independently; more specific supported scope prefixes
win, with later rules breaking ties. Child rules follow included rules.

Includes must resolve inside the selected file's directory (including symlink
resolution). If a theme needs a parent-directory dependency, select/export a
self-contained theme or put its dependencies in a common directory. Cycles,
non-regular files, non-UTF-8 input, invalid mapped colors and external `.tmTheme`
tokenColors paths are rejected with a diagnostic. Limits: 2 MiB per file, 8 MiB
total, 32 dependency reads, include depth 8, 16384 token rules and 128 catalog
entries. The importer does not deserialize executable extension manifests.

Declared light/dark types filter the menus. Untyped files can be selected for
either mode; missing editor colors use that mode's built-in defaults. A declared
appearance mismatch keeps the last-good asset for the requested mode, or falls
back to built-in colors. `hc`/`hcLight` map to dark/light, without claiming a
separate high-contrast accessibility mode.

Editor background and foreground, gutter background, normal/active line numbers,
selection, caret, whitespace and active-line background map to their corresponding
Jaywalk editor roles. Hex RGB/RGBA, including short forms, are supported. A
transparent editor background composites onto the fixed built-in code base
(`#121212` dark / `#f7f7f7` light), never onto the generated shell. Other supported
colors retain alpha. Explicit canonical editor overrides remain final. Workbench
shell colors and semantic diff colors are intentionally independent of imports.

## Syntax coverage

Jaywalk keeps its Tree-sitter grammars. This table maps representative TextMate
scopes to the toolkit's fixed capture palette; it does not emulate TextMate scope
stacks. Missing rules use supported broader prefixes, then the imported theme's
normal foreground, rather than colors from another theme. Supported grammars
continue highlighting even when a theme omits a rule. Unsupported languages use
plain text as before.

| Tree-sitter category | Representative TextMate scope |
| --- | --- |
| keyword | keyword, keyword.control, storage.type, storage.modifier |
| operator | keyword.operator |
| function | entity.name.function, support.function |
| constructor | entity.name.function.constructor, support.class |
| type / enum | entity.name.type / entity.name.type.enum |
| variable / special variable | variable / variable.language |
| property | variable.other.property, support.type.property-name |
| constant / boolean / number | constant / constant.language.boolean / constant.numeric |
| comment / documentation | comment / comment.block.documentation |
| string / regex / escape | string / string.regexp / constant.character.escape |
| tag / attribute | entity.name.tag / entity.other.attribute-name |
| punctuation / bracket / delimiter | punctuation / punctuation.section / punctuation.separator |
| title / emphasis / strong | markup.heading / markup.italic / markup.bold |
| inline code / literal | markup.inline.raw / markup.raw |
| link text / URI | markup.underline.link / markup.underline.link.image |

The full deterministic table is `SCOPE_MAP` in `src/theme/assets/vscode.rs`.
Context stacks, exclusions, language-specific scopes and semanticTokenColors have
no exact representation in this fixed palette and are reported as unsupported.
Underline/strikethrough are diagnosed and omitted; font family and size are not
imported. This can differ visibly from VS Code's TextMate and LSP rendering.

Reference: https://code.visualstudio.com/api/extension-guides/color-theme
and https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide.
