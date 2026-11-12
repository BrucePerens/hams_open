# The Bench Server Explains Itself Through Its Own Manual, Refuses to Open the Wrong Serial Port, and Tests Leave No Scratch Files Behind

As the person (or the AI agent) running the bench, I want `bench_mcp` to ship its documentation inside the server so I can read it through the same MCP connection I use for the instruments, I want a serial device that belongs to something else (a relay board, another instrument) never to be opened by a bench tool by accident, and, as a developer of the crate, I want every test's scratch directory to be removed when the test ends. This story documents every anchor of three groups: the manual framework (`daemons/bench_mcp/src/manual.rs`, the page parser, the argument-table generator, the BM25 (a text-relevance ranking function) search, the five `bench_manual_*` tools and the MCP resources), the serial-port guard (`src/serial_guard.rs`) and the test scratch directories (`src/testdir.rs`, compiled only for tests). The content of the manual pages themselves is under `daemons/bench_mcp/manual/`, and the contributor rule for adding a page is in the README section "The manual".

Facts that apply to every entry. Nothing here can transmit: the manual functions only read text embedded in the binary, the serial guard only looks at file names and `sysfs` entries and never opens a device, and `testdir` makes and removes directories under the system temporary directory. So the tier, the per-instrument lock, the band check and the transmit gate do not apply to any function below, and no entry keys a transmitter or touches an instrument. The five manual tools are listed whatever instruments are configured, are tagged `[ReadOnly]` and are dispatched by `main.rs` (`call`) before any instrument module sees the name. The manual is built once per process and is read-only afterwards, so it needs no lock beyond the `OnceLock` that builds it. Errors are `String`s (the MCP layer turns them into a tool error) except where an entry says `McpError`.

## The embedded manual: files, front matter and page paths

### `embedded` module [@ANCHOR: bench_mcp_manual_embedded_paths]

The module that holds the table of every manual file, generated at build time. `build.rs` walks `daemons/bench_mcp/manual/` and writes `$OUT_DIR/manual_files.rs`, a constant `FILES: &[(&str, &str)]` of `(path relative to manual/, file text)` pairs in which each text is an `include_str!`. The module only `include!`s that generated file, so the installed server needs no `manual/` directory at run time. The comment in the code records the one rule that matters: each `include_str!` is built from `CARGO_MANIFEST_DIR` plus the relative path, never from an absolute path, so a build in a different checkout or worktree still finds the same files.

| Item | Type | Meaning |
|---|---|---|
| `FILES` | `&[(&str, &str)]` | Relative file name with forward slashes (for example `tools/bench_vna_sweep.md`), then the whole file text including its front matter. Order is the directory walk order of `build.rs` (the manual sorts by page path itself). |

Read by `manual()` (through `Manual::build`) and nowhere else. A manual file added, removed or edited needs a rebuild; a non-`.md` file in the tree is reported by `path_of_file` when the manual is first built. Tests: `bench_mcp_manual_embedded_paths` is cited in `src/manual/tests.rs`. See also `Manual::build`.

### `split_front_matter` [@ANCHOR: bench_mcp_manual_split_front_matter]

Splits the text of one manual file into its front-matter lines and its body. Private to `manual.rs`, called by `parse_page` for every file when the manual is built.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | The whole file. It must start with a line that is exactly `---` (trailing white space allowed). |

Returns `Ok((pairs, body))`. `pairs` is a `Vec<(String, String)>` of `key`, `value` in file order, both trimmed; a line is split at its first colon, so a value may contain colons. Blank lines inside the front matter are skipped. The closing line is another `---`. `body` is everything after the closing line, joined with `\n`, with leading newlines removed (so a body never starts with a blank line).

Errors, all `Err(String)` and prefixed with the file name by `parse_page`: the first line is not `---` ("no front matter: the file must start with a --- line"); a non-blank line without a colon ("front matter line without a colon"); no closing `---` ("front matter is not closed by a --- line"). It does not check the keys: `parse_page` does. Pure function, no I/O. Tests: `bench_mcp_manual_split_front_matter` in `src/manual/tests.rs`.

### `parse_list` [@ANCHOR: bench_mcp_manual_parse_list]

Turns the value of a list-valued front-matter key (`see_also`, `tools`, `instrument`) into a vector of strings. Private; called by `parse_page` through a small closure for the three keys in `LIST_KEYS`.

| Argument | Type | Meaning |
|---|---|---|
| `key` | `&str` | The key name, used only in the error text. |
| `v` | `&str` | The already trimmed value: `[a, b, c]`, a single bare word, or empty. |

Returns `Ok(Vec<String>)`: for `[a, b]` the comma-separated, trimmed, non-empty items; for an empty value an empty vector; for any other value (no leading `[`) a one-element vector holding the value as written, so `tools: bench_x` works without brackets. A bracket list is not nested and an item may not contain a comma. Error: a value starting with `[` that does not end with `]` gives `"<key>: list without a closing ]"`. Pure function. Tests: `bench_mcp_manual_parse_list` in `src/manual/tests.rs`.

### `path_of_file` [@ANCHOR: bench_mcp_manual_path_of_file]

Computes the page path, the directory and the index flag of a manual file from its file name. Private; called by `parse_page`.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&str` | A name relative to `manual/`, with forward slashes, for example `instruments/vna/index.md`. |

Returns `Ok((path, dir, is_index))` as `(String, String, bool)`. The path is the name without `.md`. A file named `index.md` is the page of its directory: its path is the directory's path (`instruments/vna/index.md` becomes `instruments/vna`) and `is_index` is true; the top file `index.md` has the path `index` and an empty `dir`. `dir` is everything before the last slash, empty for a file at the root. Error: a name that does not end in `.md` gives `"<file>: only .md files belong in the manual"`, which fails the whole build of the manual. Pure function. Tests: `bench_mcp_manual_path_of_file` in `src/manual/tests.rs`.

### `parse_page` [@ANCHOR: bench_mcp_manual_front_matter]

Parses one manual file into a `Page`, and enforces the front-matter rules that the manual's tests rely on. Public so the tests can parse sample files; called by `Manual::build` once per embedded file.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&str` | The file name relative to `manual/` (decides path, directory and index flag through `path_of_file`). |
| `text` | `&str` | The whole file text. |

Returns `Ok(Page)`, whose fields are the data structure of a page (`Page` has no anchor of its own):

| Field | Type | Meaning |
|---|---|---|
| `path` | `String` | Path from the root without `.md`; a directory's index page has the directory's path; the top page is `index`. |
| `dir` | `String` | The directory the file is in; empty for the root. |
| `is_index` | `bool` | True for an `index.md`. Index pages list their children. |
| `title` | `String` | Front-matter `title`; empty if missing (the tests require it). |
| `summary` | `String` | Front-matter `summary`, one line; empty if missing. |
| `see_also` | `Vec<String>` | Page paths from the root of the manual, no `.md`. |
| `tools` | `Vec<String>` | Tool names the page documents; the first one drives the generated argument table. |
| `instruments` | `Vec<String>` | From the key `instrument`: names of directories under `instruments/` the page is about. |
| `tier` | `Option<String>` | `read_only`, `read_config` or `full`, or `None` when the key is absent. |
| `verified` | `String` | What was checked on real hardware and when; empty if missing (the tests require it and require an honest value). |
| `body` | `String` | The text after the front matter, with the two markers still in it. |

Errors (each prefixed with the file name): a file name that `path_of_file` refuses; any front-matter error of `split_front_matter`; a key not in `KEYS` (`title`, `summary`, `see_also`, `tools`, `instrument`, `tier`, `verified`); the same key twice; a `tier` that is not one of `read_only`, `read_config`, `full`; a list error from `parse_list`. It does not check that the required keys are present, that paths resolve or that tools exist: the tests do (see the entries for `bench_mcp_manual_tool_pages`, `bench_mcp_manual_links`, `bench_mcp_manual_index_pages` and `bench_mcp_manual_markdown`). Pure function. Tests: `bench_mcp_manual_front_matter` in `src/manual/tests.rs`.

### `normalize` [@ANCHOR: bench_mcp_manual_links]

Turns whatever page reference a caller typed into the canonical page path. It is the single place that decides which spellings of a path are the same page, and the anchor name is about the manual's links: every `see_also` entry, every `[text](target)` link and every `path` argument goes through it. Public; called by `Manual::get`, `Manual::index_of` and `not_found`.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | A page path as a person or an agent wrote it. |

Returns a `String`. In order it trims white space, removes a leading `bench-manual://`, removes slashes at both ends, removes a trailing `.md`, removes a trailing `/index`, and returns `index` if nothing is left. So `bench-manual://instruments/vna/`, `/instruments/vna/index.md` and `instruments/vna` all mean `instruments/vna`, and `""` means the top page. It never fails and never checks that the page exists; the top page is `index`, so a directory literally named `index` below the root would be ambiguous (none exists; unknown whether anything prevents one). Pure function. Tests: `bench_mcp_manual_links` in `src/manual/tests.rs` (the link-resolution test).

## The page tree

### `Manual::build` [@ANCHOR: bench_mcp_manual_manual_build]

Builds the whole in-memory manual from a list of files: parses every page, rejects a duplicate path, renders every body and builds the search index. Public so tests can build a small manual; `manual()` calls it once with `embedded::FILES` and caches the result.

| Argument | Type | Meaning |
|---|---|---|
| `files` | `&[(&str, &str)]` | `(file name relative to manual/, file text)` pairs, in any order. Borrowed. |

Returns `Ok(Manual)`. The `Manual` struct holds `pages` (`Vec<Page>`, sorted by path), `by_path` (path to index, private), `rendered` (the served text of each page with the two markers replaced) and `search` (the BM25 index of `SearchIndex`). It is never changed after `build` returns, so it is shared freely between threads. Errors: any `parse_page` error, or `"two pages have the path <path>"` (for example `a.md` and `a/index.md`). Cost: parses and renders every page and indexes every word once (the time for the real manual is unknown, not measured here); done lazily on the first manual call. Tests: through `bench_mcp_manual_index_pages` and the others in `src/manual/tests.rs`. See also `manual()` and `SearchIndex`.

### `Manual::children_of` [@ANCHOR: bench_mcp_manual_index_pages]

Lists what an index page shows below it. Used for the `<!-- children -->` marker, for the `children` field of `bench_manual_read`, for the section list of `bench_manual_index` and for `bench_manual_for_tool`, and by the tests that check that every directory has an index page that lists all its children.

| Argument | Type | Meaning |
|---|---|---|
| `page` | `&Page` | Any page. Only an index page has children. |

Returns `Vec<&Page>`: for a non-index page an empty list. For an index page: the pages in the same directory that are not index pages, plus the index pages of the directories directly below it. The page itself is excluded and deeper levels are not included. Order: sub-directory indexes first, then plain pages, each group by path. Pure; no side effects. Tests: `bench_mcp_manual_index_pages` in `src/manual/tests.rs`.

### `Manual::up_of` [@ANCHOR: bench_mcp_manual_manual_up_of]

Finds the page that lists a given page, the "up" link of `bench_manual_read`.

| Argument | Type | Meaning |
|---|---|---|
| `page` | `&Page` | The page whose parent is wanted. |

Returns `Option<&Page>`. The top page (`index`) has no parent, so `None`. A plain page's parent is the index page of its directory (the top page when the page is at the root). An index page's parent is the index page of the directory one level up. `None` also when that parent page does not exist (the tests require every directory to have an index, so this is not expected). Pure; no side effects. Tests: exercised through `bench_manual_read` (`bench_mcp_manual_read`).

### `Manual::render_body` [@ANCHOR: bench_mcp_manual_manual_render_body]

Produces the text of one page as served, by replacing the two markers in its body. Private; called once per page by `Manual::build`, so rendering costs nothing at read time.

| Argument | Type | Meaning |
|---|---|---|
| `i` | `usize` | Index into `pages`. Must be in range (it panics otherwise; the caller loops over `0..pages.len()`). |

Returns the rendered `String`. `<!-- arguments -->` is replaced by `arguments_table` of the page's first `tools` entry, or by "(this page documents no tool)" when the page lists none. `<!-- children -->` is replaced by one line per child, `* [title](path) - summary`, with a trailing `/` after the link for a directory's index page, or by "(nothing below this page yet)". A page without markers is returned unchanged. Because the table is generated from the registered tool's JSON schema, a manual page cannot disagree with the code about argument names or types. No errors. Tests: `bench_mcp_manual_arguments` and `bench_mcp_manual_index_pages` in `src/manual/tests.rs`.

## Argument tables generated from the tool schemas

### `tool_registry` [@ANCHOR: bench_mcp_manual_tool_registry]

Builds, once, the map of every tool the server can register, with its tier and its JSON schema, for the argument tables and the manual tests. Private; `registered_tools` returns it. The data structure is `ToolInfo` (no anchor of its own):

| Field | Type | Meaning |
|---|---|---|
| `tier` | `String` | `read_only`, `read_config` or `full`, taken from the `[ReadOnly`, `[ReadConfig` or `[Full` prefix of the tool description; `unknown` if the description has no such prefix. |
| `schema` | `Arc<Map<String, Value>>` | The tool's `input_schema` (an object schema with `properties` and `required`). |

The map is a `BTreeMap<String, ToolInfo>` keyed by tool name, so it iterates in name order. It is made from `crate::all_tool_defs()`, which lists every tool whatever the config says, held in a `OnceLock`, never changed afterwards and thread safe. The tier is recovered from the description text, so a tool whose description does not start with a tier bracket shows `unknown` and fails the manual tests. No errors, no I/O. Tests: `bench_mcp_manual_tool_registry` is cited from `src/checklist.rs` (the tests that every tool has a page).

### `registered_tools` [@ANCHOR: bench_mcp_manual_tool_pages]

The public view of the tool registry: every registered tool and its tier and schema, whatever instruments are configured. Called by `bench_manual_for_tool` (to say whether a name is a registered tool and to suggest similar names) and by the manual tests, which check that every registered tool has a page under `tools/` carrying the tool's own tier and that every `tools:` entry of a page names a registered tool.

It takes no arguments and returns `&'static BTreeMap<String, ToolInfo>` (see `tool_registry`). It cannot fail; the first call builds the map. Pure after that. Tests: `bench_mcp_manual_tool_pages` in `src/manual/tests.rs`.

### `type_of` [@ANCHOR: bench_mcp_manual_type_of]

Names the type of one argument for the "Type" column of an argument table. Private; called by `arguments_table`.

| Argument | Type | Meaning |
|---|---|---|
| `prop` | `&Value` | One entry of the schema's `properties`. |

Returns a `String`: the JSON-schema `type` as written (`string`, `integer`, `number`, `boolean`, `object`); `array of <item type>` for an array (`array of any` when the item type is missing); `any` when there is no `type` key (for example a schema written with `oneOf`, whose alternatives are not described here). Pure function. Tests: `bench_mcp_manual_type_of` in `src/manual/tests.rs`.

### `notes_of` [@ANCHOR: bench_mcp_manual_notes_of]

Builds the "Values and notes" cell for one argument from its schema entry, so valid values, range, default and description come from the code and cannot drift. Private; called by `notes_for`.

| Argument | Type | Meaning |
|---|---|---|
| `prop` | `&Value` | One entry of the schema's `properties`. |

Returns a `String` made of these parts, in this order, joined with `; `: `one of` and the backticked values of `enum`; `each one of` and the values of `items.enum`; `minimum`, `maximum` and `default` followed by the value, for each that is present; and the schema's `description`. A string enum value is shown without quotes, any other value in its JSON form. An entry with none of these gives the empty string. Pure function. Tests: `bench_mcp_manual_notes_of` in `src/manual/tests.rs`.

### `notes_for` [@ANCHOR: bench_mcp_manual_notes_for]

The notes cell for a named argument: `notes_of`, with one fallback so every table explains the `instrument` argument. Private; called by `arguments_table`.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The argument name. |
| `prop` | `&Value` | Its schema entry. |

Returns the `notes_of` text; when the argument is named `instrument` and that text is empty, the constant `INSTRUMENT_NOTE`: "the alias of the configured instrument (see bench_list); may be left out when exactly one configured instrument fits the tool". No errors. Pure function. Tests: `bench_mcp_manual_notes_for` in `src/manual/tests.rs`.

### `arguments_table` [@ANCHOR: bench_mcp_manual_arguments]

Generates the markdown argument table of one tool from its registered JSON schema. It is what replaces `<!-- arguments -->` in a tool page, and the manual tests also call it to check that every argument is explained on the page.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `&str` | A registered tool name, for example `bench_vna_sweep`. |

Returns a `String`. For an unknown tool the text `(no registered tool named <tool>)`, not an error. For a schema without properties `This tool takes no arguments.` Otherwise a table with the header `| Argument | Required | Type | Values and notes |` and one row per property in schema order: the name in backticks, `yes` when the name is in the schema's `required` list (else `no`), the text of `type_of` and the text of `notes_for`. In a cell a `|` is escaped and a line break becomes a space. Pure function over the registry. Tests: `bench_mcp_manual_arguments` in `src/manual/tests.rs`.

### `with_pointers` [@ANCHOR: bench_mcp_manual_pointers]

Appends the manual path to every tool description, so an agent reading the tool list knows where its page is. Public; `main.rs` applies it once to the complete tool list (the instrument tools, the module tools and the manual tools) where `tools/list` is assembled.

| Argument | Type | Meaning |
|---|---|---|
| `tools` | `Vec<Tool>`, owned | The finished tool list. |

Returns the same tools, each description now ending with ` Manual: tools/<tool name>` (the old description is trimmed of trailing white space first; a tool with no description gets the pointer with a leading space). The input list is consumed. It does not check that the page exists; the manual tests check both that every description ends with the pointer and that the page exists. Pure function. Tests: `bench_mcp_manual_pointers` in `src/manual/tests.rs`.

## The search index

### `tokens` [@ANCHOR: bench_mcp_manual_tokens]

Splits text into the lower-case words used for indexing and for queries. Private; used by `SearchIndex::build`, by `search` and by `excerpt`, so a query and a page are cut into words the same way.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | Any text (a page, a title, a query). |

Returns `Vec<String>`. The text is lower-cased and split at every character that is not alphanumeric and not `_`; leading and trailing underscores are trimmed from a word; empty words are dropped. A word that contains an underscore (a tool or argument name, such as `bench_vna_sweep`) is returned whole and then again as its parts (`bench`, `vna`, `sweep`), so a search for `vna` finds tool pages and a search for the full name ranks the exact page. Duplicates are kept (term frequency counts them). Pure function. Tests: through `bench_mcp_manual_search` in `src/manual/tests.rs`.

### `SearchIndex::build` [@ANCHOR: bench_mcp_manual_searchindex_build]

Builds the BM25 index of the manual. Private; called once by `Manual::build` after all pages are rendered. `SearchIndex` (no anchor of its own) holds `docs` (a map word to weighted count for each page, `Vec<HashMap<String, f32>>`), `lens` (each page's total weighted word count), `avg_len` (their mean) and `df` (the number of pages containing each word).

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Manual` | The manual, with `pages` and `rendered` filled in. |

Returns a `SearchIndex`. A word's count on a page is weighted by where it appears: title 3.0, `tools` entries 3.0, summary 2.0, path 2.0, `instruments` entries 2.0 and the rendered body 1.0, so a page about a tool outranks a page that only mentions it. `avg_len` is the mean of `lens` (the divisor is at least 1, so an empty manual gives 0, not a panic). No errors, no I/O. Cost is one pass over every page, once per process. Tests: through `bench_mcp_manual_search` in `src/manual/tests.rs`.

### `SearchIndex::score` [@ANCHOR: bench_mcp_manual_searchindex_score]

The BM25 score of one page for a query.

| Argument | Type | Meaning |
|---|---|---|
| `doc` | `usize` | Index into the page list; must be in range (panics otherwise). |
| `query` | `&[String]` | The query words, from `tokens`. A repeated word counts once per repetition. |

Returns an `f32` of 0.0 or more; 0.0 means no query word is on the page. For each query word present on the page: `idf * tf * (k1 + 1) / (tf + k1 * (1 - b + b * len / avg_len))`, with `k1 = 1.5`, `b = 0.75`, `idf = ln(1 + (N - df + 0.5) / (df + 0.5))`, `N` the number of pages, `tf` the weighted count (see `SearchIndex::build`) and `len` the page's weighted length. The scores of the words are summed. Higher is better; the value has no unit and is only comparable within one query. Pure function. Tests: through `bench_mcp_manual_search` in `src/manual/tests.rs`.

### `excerpt` [@ANCHOR: bench_mcp_manual_excerpt]

Picks the paragraph of a page that shows best why it matched, with the heading it sits under. Private; called by `search` for each result.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The rendered page text. |
| `query` | `&[String]` | The query words, from `tokens`. |

Returns `(excerpt, heading)`, both `String`. The page is cut into paragraphs at blank lines, and a heading line (a line starting with `#`, outside a code fence) also ends a paragraph and becomes the current heading; a code fence is kept together as one paragraph. The paragraph with the most distinct query words wins and, on a tie, the earlier one; the excerpt is that paragraph cut by `clip_around` to 320 characters, and the heading is the one it sat under. If no paragraph contains a query word (the page matched on its title or metadata only), the excerpt and heading are empty strings. No errors. Pure function. Tests: through `bench_mcp_manual_search` in `src/manual/tests.rs`.

### `clip_around` [@ANCHOR: bench_mcp_manual_clip_around]

Shortens a long paragraph to a window around the first query word.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The paragraph. |
| `query` | `&[String]` | The query words. |
| `max` | `usize` | The window length in characters (the caller uses 320). |

Returns a `String`. A text of `max` characters or fewer is returned unchanged. Otherwise the window starts 60 characters before the earliest match of any query word (at the start if none matches), is `max` characters long (less near the end), and is marked with `... ` in front when it does not start at the beginning and ` ...` at the end when it does not reach the end, so the result can be a little longer than `max`. The match position is found in the lower-cased text, so a character whose lower-case form has a different length can shift the window by a few characters; the window itself is cut on character boundaries. Oddity (reported, not changed): the match position is a byte offset in the lower-cased copy but is used to slice the original text, so a text whose lower-case form changes byte length before the match could in principle make that slice panic; whether any manual text does this is unknown. Pure function. Tests: `bench_mcp_manual_clip_around` in `src/manual/tests.rs`.

## Reading a page: headings, sections and cutting

### `headings` [@ANCHOR: bench_mcp_manual_markdown]

Lists the markdown headings of a page text, outside code fences. It is the page-structure check behind "pages are valid markdown of bounded length" (the tests use it to require one `# title` heading and to limit page size), and it feeds `section_of` and the `headings` field of `bench_manual_read`. Private to `manual.rs`; the `Heading` struct (`level`, `text`, `line`) is its result type.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | A page text. |

Returns `Vec<Heading>` in page order. A line counts when it starts with 1 to 6 `#` characters followed by a space; a line inside a fenced block (a line starting with three backticks toggles the fence) never counts, and the fence lines themselves are skipped. `level` is the number of `#`, `text` the trimmed heading text, `line` the zero-based line number. No errors. Pure function. Tests: `bench_mcp_manual_markdown` in `src/manual/tests.rs`.

### `section_of` [@ANCHOR: bench_mcp_manual_section_of]

Returns one section of a page: the heading that matches a given name and everything under it. Private; called by `read` when the `section` argument is given.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The rendered page text. |
| `want` | `&str` | A heading name; leading `#` characters and white space are ignored and case does not matter. |

Returns `Ok(String)`: the lines from the matching heading up to, not including, the next heading of the same or a higher level (or to the end of the page). The heading is chosen by trying, in order, an exact match, a prefix match and a substring match, and taking the first heading found for the first rule that matches anything. Error: no heading matches, and the message lists every heading of the page separated by ` | `, so the caller can retry. Pure function. Tests: through `bench_mcp_manual_read` in `src/manual/tests.rs`.

### `truncate_chars` [@ANCHOR: bench_mcp_manual_truncate_chars]

Cuts a long text to a character limit without cutting a table row in two. Private; called by `read` for the `max_chars` limit.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The text to limit. |
| `max` | `usize` | Limit in characters (not bytes). |

Returns `(String, bool)`: the text and whether it was cut. Text of `max` characters or fewer is returned unchanged with `false`. Otherwise the first `max` characters are taken, and if a line break lies in the last quarter of that piece (after three quarters of `max`) the cut moves back to that line break, otherwise it is exactly at `max`. The cut text is never longer than `max`. No errors; cannot split a multi-byte character. Pure function. Tests: cited from tests that exercise `bench_manual_read` (`bench_mcp_manual_read`), and from `src/checklist.rs` (see the story `procedure_store_checklist.md`).

## The five manual tools

The tools come from the constant table `SPECS` (name, description, JSON property map, required list; no anchor of its own). Each tool is listed with the prefix `[ReadOnly]`. `tool_defs()` builds the `Tool` objects for `tools/list` and `is_manual_tool(name)` tells `main.rs` whether a name is one of them; neither has an anchor of its own.

### `str_arg` [@ANCHOR: bench_mcp_manual_str_arg]

Reads an optional string argument from the tool's JSON arguments. Private; used by `read`, `search`, `for_tool` and `whats_new`.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The arguments object. |
| `key` | `&str` | The argument name. |

Returns `Ok(Some(&str))` (borrowed from `args`) for a string, `Ok(None)` when the key is missing or `null`. Error: any other JSON type gives `"<key> must be a string"`. An empty string is returned as `Some("")`, not `None`. Pure function.

### `uint_arg` [@ANCHOR: bench_mcp_manual_uint_arg]

Reads an optional whole-number argument within a range. Private; used by `read` (`max_chars`) and `search` (`limit`).

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The arguments object. |
| `key` | `&str` | The argument name. |
| `min`, `max` | `u64` | The inclusive limits. |

Returns `Ok(Some(usize))`, or `Ok(None)` when the key is missing or `null`. Error: a value that is not a non-negative JSON integer, or is outside `min..=max`, gives `"<key> must be an integer from <min> to <max>"` (a float such as `5.0` and a negative number are both refused). Pure function.

### `call` [@ANCHOR: bench_mcp_manual_call]

The dispatcher for the manual tools. `main.rs` (`call`) hands it any tool name for which `is_manual_tool` is true, after adding the `device` argument handling, and before looking for an instrument module. It runs on the server's call path, takes no instrument lock (it touches no instrument) and does only in-memory work, so it never blocks for more than the first-use build of the manual.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | `bench_manual_index`, `bench_manual_read`, `bench_manual_search`, `bench_manual_for_tool` or `bench_manual_whats_new`. |
| `args` | `&Value` | The tool arguments (an object; each tool says which keys it reads). |

Returns `Ok(Value)`, the JSON result of the tool (the entries below give each shape), or `Err(String)` from the tool. Other errors: the manual cannot be built (the error of `Manual::build`, which would also make every manual test fail), and `"unknown tool <name>"` for any other name. Tests: through the tool tests (`bench_mcp_manual_index`, `_read`, `_search`, `_for_tool`, `_whats_new`).

### `index` (`bench_manual_index`) [@ANCHOR: bench_mcp_manual_index]

The tool `bench_manual_index`: the top of the manual with one-line summaries, where an agent starts. It takes no arguments.

Returns a JSON object:

| Field | Type | Meaning |
|---|---|---|
| `start` | string | The summary of the top page. |
| `read_with` | string | A fixed hint on which tool to call next. |
| `pages_total` | integer | The number of pages in the manual. |
| `sections` | array | One object per child of the top page: `path`, `title`, `summary`; for a directory also `page_count` (its direct children) and, except for the `tools` directory, `children` (their `path`, `title`, `summary`). The `tools` directory gives only its count, because it lists every tool. |

The function panics (`expect`) only if the manual has no top `index` page, which the tests prevent. Pure; no side effects. Tests: `bench_mcp_manual_index` in `src/manual/tests.rs`.

### `read` (`bench_manual_read`) [@ANCHOR: bench_mcp_manual_read]

The tool `bench_manual_read`: one manual page by path, optionally one section, optionally cut to a length.

| Argument | Type | Meaning |
|---|---|---|
| `path` | string, required | A page path from the root, without `.md`; `index` or an empty string is the top page; a directory's path gives its index page. `normalize` accepts a `bench-manual://` prefix, slashes and `.md`. |
| `section` | string, optional | A heading of the page: case-insensitive, a prefix or part of a heading is enough (see `section_of`). |
| `max_chars` | integer, optional | 200 to 1,000,000, default 24,000 (`DEFAULT_MAX_CHARS`). Counted in characters. |

Returns an object: `path`, `uri` (`bench-manual://<path>`), `title`, `summary`, `instrument` (list), `tier` (or null), `tools` (list), `verified`, `see_also` (the pages named by the page and found, each as `path`, `title`, `summary`; one that does not exist is silently left out), `children` (as `children_of`), `headings` (the headings of the whole page, even when only a section is returned), `text` (the rendered page or section, possibly cut), `chars_total` (length of the text before cutting, in characters), `truncated` (bool) and, when cut, `note` saying how much was cut and how to get the rest; also `up` (the parent page, `up_of`) when there is one.

Errors: `path` missing ("path is required (use index for the top page)"); a wrong argument type or range (`str_arg`, `uint_arg`); an unknown page, with up to eight similar page paths (`not_found`); an unknown section (all headings listed, `section_of`). Pure function over the manual, no side effects. Tests: `bench_mcp_manual_read` in `src/manual/tests.rs`.

### `not_found` [@ANCHOR: bench_mcp_manual_not_found]

Builds the error text for a page path that does not exist, with suggestions. Private; called by `read` and `read_resource`.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Manual` | The manual. |
| `path` | `&str` | The path as the caller wrote it (shown in the message as written, and normalized for the comparison). |

Returns a `String`: `no manual page "<path>"; try: <paths>`. The paths are up to eight pages (in path order) whose path contains the normalized path, or is contained in it, or whose last path segment equals its last segment; when none is found, the paths of all index pages (the directory list). Pure function. Tests: through `bench_manual_read` (`bench_mcp_manual_read`).

### `search` (`bench_manual_search`) [@ANCHOR: bench_mcp_manual_search]

The tool `bench_manual_search`: full-text search of the manual, best pages first, each with the matching excerpt.

| Argument | Type | Meaning |
|---|---|---|
| `query` | string, required | The words to find. Cut into words by `tokens`; a name with underscores also matches its parts. |
| `instrument` | string, optional | Only pages whose `instrument` list contains this name (a directory name under `instruments/`, for example `vna`). |
| `tool` | string, optional | Only pages whose `tools` list contains this tool name. |
| `limit` | integer, optional | 1 to 30, default 8: the number of results returned. |

Returns `{"query": <as given>, "matches": <number of pages with a score above 0>, "results": [...]}`; each result has `path`, `title`, `summary`, `score` (BM25 from `SearchIndex::score`, rounded to two decimals), `section` (the heading of the excerpt) and `excerpt`. Order: score descending, ties by path. A query with no hits gives `matches` 0 and an empty list, not an error. Errors: `query` missing ("query is required") or with no words ("query has no words"); a wrong argument type or range. Pure function. Tests: `bench_mcp_manual_search` in `src/manual/tests.rs`.

### `for_tool` (`bench_manual_for_tool`) [@ANCHOR: bench_mcp_manual_for_tool]

The tool `bench_manual_for_tool`: the manual page of one tool and the instrument pages that apply to it.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | string, required | A tool name such as `bench_vna_sweep`. |

Returns an object: `tool`; `registered` (whether the name is a registered tool); `tier` (the registered tier, or null); `page` (the page `tools/<tool>` as `path`, `title`, `summary`, or null); `also_documented_in` (other pages whose `tools` list names the tool); `instrument_pages` (for every instrument named by the tool page or those pages, the index page of `instruments/<name>` followed by its children, each once); `concepts` (always `concepts/safety-tiers` and `concepts/errors`); and `read_with` (a ready `bench_manual_read` call).

Error: `tool` missing ("tool is required"), or a name for which there is neither a page nor a documenting page. The message then lists up to six registered tools that look similar: names that share at least two words with the wrong name, where words are the parts between underscores and two words match when they share a long enough common prefix (a typo such as a plural or a missing letter still matches). Pure function. Tests: `bench_mcp_manual_for_tool` in `src/manual/tests.rs`.

### `releases` [@ANCHOR: bench_mcp_manual_releases]

Parses the `whats-new` page into releases. Public so the tests can use it; called by `whats_new`.

| Argument | Type | Meaning |
|---|---|---|
| `body` | `&str` | The body (after the front matter) of the page `whats-new`. |

Returns `Vec<(version, Vec<(path, note)>)>` in page order (newest first, as the page is written). A line beginning `## ` starts a release and gives its version text (trimmed). A line, after any indentation, beginning `- ` is an item of the latest release: the text before the first colon is the page path and the rest the note (both trimmed; with no colon the whole item is the path and the note is empty). Items before the first `## ` and other lines (blank lines, prose, the `#` title) are ignored. No errors. Pure function. Tests: through `bench_mcp_manual_whats_new` in `src/manual/tests.rs`, which also checks that every listed page exists.

### `whats_new` (`bench_manual_whats_new`) [@ANCHOR: bench_mcp_manual_whats_new]

The tool `bench_manual_whats_new`: the manual pages added or changed in each build version, newest first, with a note for each.

| Argument | Type | Meaning |
|---|---|---|
| `version` | string, optional | One release as written in the page (for example `0.1.0`); default all releases. Must match exactly. |

Returns `{"build_version": <the crate version, compiled in>, "releases": [...]}`; a release is `{"version", "pages": [{"path", "exists", "title", "note"}]}`, where `exists` says whether the path is still a page and `title` is that page's title or null. Errors: the manual has no `whats-new` page; a `version` that matches no release, with the list of releases. Pure function. Tests: `bench_mcp_manual_whats_new` in `src/manual/tests.rs`.

## The manual as MCP resources

### `page_text_for_resource` [@ANCHOR: bench_mcp_manual_page_text_for_resource]

Makes the text a client receives when it reads a page as a resource: a short header, then the rendered page. Private; called by `read_resource`.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Manual` | The manual (for the rendered body). |
| `p` | `&Page` | The page. |

Returns a `String`: the title on the first line; `Summary: <summary>`; `Tier: <tier>` only when the page has a tier; `Verified: <verified>`; a blank line; then the rendered body (markers replaced). A page that cannot be found in `m` gives an empty body (not expected). Pure function. Tests: `bench_mcp_manual_page_text_for_resource` in `src/manual/tests.rs`.

### `list_resources` [@ANCHOR: bench_mcp_manual_resources]

Answers the MCP `resources/list` request: every manual page as a resource, so a client that shows resources lists the whole manual. Called by the `list_resources` handler of the server in `main.rs`; the request's cursor is ignored (everything is one page of results).

It takes no arguments and returns `Result<ListResourcesResult, McpError>`: one `Resource` per page, in path order, with the URI `bench-manual://<path>`, the path as its name, the title, the summary as description and MIME type `text/markdown`. Error: if the manual cannot be built, an `McpError` of kind internal error with the build message. No side effects. Tests: `bench_mcp_manual_resources` in `src/manual/tests.rs`. Some MCP clients ignore resources; the tools are the primary route.

### `read_resource` [@ANCHOR: bench_mcp_manual_read_resource]

Answers the MCP `resources/read` request for one page.

| Argument | Type | Meaning |
|---|---|---|
| `uri` | `&str` | `bench-manual://<path>`; the path is normalized like a tool's `path`, so a trailing `.md` or slash is tolerated. |

Returns `Result<ReadResourceResponse, McpError>` holding one text content with the URI as given, MIME type `text/markdown` and the text of `page_text_for_resource`. Errors: a URI without the prefix `bench-manual://` gives an invalid-params error; an unknown page gives a resource-not-found error whose message is the `not_found` text with suggestions; an unbuildable manual gives an internal error. Pure over the manual, no side effects. Tests: `bench_mcp_manual_read_resource` in `src/manual/tests.rs`.

## Which serial devices a tool may open

`serial_guard.rs` exists because `/dev/ttyUSBn` numbers follow plug-in order: a bare `ttyUSB0` can silently become the SainSmart relay board or another instrument after a reboot or a replug, and a bench tool would then send commands to the wrong device. Two rules, both from the module's header comment: `reserved_serial` (a top-level config key, a list of devices no bench tool opens) and a supply is opened by a bare `ttyUSBn` or `ttyACMn` name only when no `/dev/serial/by-id` name points at that device. The checks only read the file system and never open the device. They are called from `link.rs`: `Serial::open` makes the reserved check for every serial device (`check_reserved_open`) and `Link::serial`, which a supply uses, makes the supply check (`check_supply_open`, which has no anchor of its own: it is `check_supply` against the process's list and the real roots).

The data structure `Roots` (no anchor of its own) holds the two directories the checks read, so the tests can use a fake tree: `by_id: &Path` (the real value is `/dev/serial/by-id`) and `sys: &Path` (`/sys`). `real_roots()` returns the real ones. Nothing here can transmit.

### `parse_reserved` [@ANCHOR: bench_mcp_serial_guard_parse_reserved]

Reads and checks the `reserved_serial` list from the top level of the parsed config.

| Argument | Type | Meaning |
|---|---|---|
| `top` | `&serde_json::Value`, borrowed | The whole config object (not one instrument entry). |

Returns `Ok(Vec<String>)`: the entries in order; an empty list when the key is missing. An entry is valid when it is either `usb:<bus-port>` where the port is not empty and made only of digits, `-` and `.` (for example `usb:1-4.2`), or a path starting with `/dev/` and containing no `..` (normally `/dev/serial/by-id/...`). Errors: the value is not a list or an entry is not a string ("config: reserved_serial must be a list of strings"); an entry that is neither form (the message names the entry). The check is on the text only; a `/dev/` path that does not exist is accepted. Called once by `config::load`, so a bad entry stops the server at start. Pure function. Tests: `bench_mcp_serial_guard_parse_reserved` in `src/serial_guard.rs`.

### `set_reserved` [@ANCHOR: bench_mcp_serial_guard_set_reserved]

Remembers the reserved list for the whole process, where `check_supply_open` and `check_reserved_open` find it.

| Argument | Type | Meaning |
|---|---|---|
| `list` | `Vec<String>`, owned | The checked list from `parse_reserved`. |

Returns nothing. It stores into a process-wide `OnceLock`, so only the first call counts: a second call is ignored without any error (in a test binary that calls `config::load` more than once the first list wins; whether any test relies on that is unknown). `config::load` calls it once, before any device is opened. Until it is called the list is empty and nothing is reserved. Thread safe; no file system access.

### `usb_port_of` [@ANCHOR: bench_mcp_serial_guard_usb_port_of]

Finds the place of a serial device in the USB tree, such as `1-4.2` (bus 1, hub port 4, device port 2), through `sysfs`. Private; called by `reserved_reason`.

| Argument | Type | Meaning |
|---|---|---|
| `dev` | `&Path` | The device, normally already resolved (its file name, for example `ttyUSB0`, is what is looked up). |
| `sys` | `&Path` | The `sysfs` root (`/sys`, or a fake tree). |

Returns `Option<String>`. It resolves `<sys>/class/tty/<name>/device` through symbolic links and takes, from the resulting path, the last component that starts with a digit and contains `-` (the part before a `:` is used, so `1-4.2:1.0` gives `1-4.2`). `None` when the device has no such `sysfs` entry (not a USB serial device, a pseudo-terminal, or a path that does not exist) or the entry has no such component. Reads the file system only.

### `reserved_reason` [@ANCHOR: bench_mcp_serial_guard_reserved_reason]

Says why a device path is on the reserved list, or that it is not.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The device path a tool wants to open (for example `/dev/serial/by-id/...` or `/dev/ttyUSB0`). |
| `reserved` | `&[String]` | The reserved list (see `parse_reserved`). |
| `r` | `&Roots` | The directories to read. |

Returns `Option<String>`: `None` when the device is free to open; otherwise a sentence naming the path and the matching entry ("... is reserved by the config's reserved_serial entry ..."). The device matches a `usb:<port>` entry when its `usb_port_of` is that port, and a path entry when the entry, resolved through symbolic links, is the same file as the device resolved the same way. A path that cannot be resolved (it does not exist) is compared by its literal name. Reads the file system only; never opens the device. Tests: `bench_mcp_serial_guard_reserved_reason` in `src/serial_guard.rs`.

### `by_id_name` [@ANCHOR: bench_mcp_serial_guard_by_id_name]

Finds the stable `/dev/serial/by-id` name of a device, if it has one.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | A device path such as `/dev/ttyUSB0`. |
| `r` | `&Roots` | The directories to read (`by_id` is searched). |

Returns `Option<PathBuf>`: the first, in sorted order, entry of the `by_id` directory that resolves to the same file as `path`; `None` when there is none or the directory cannot be read (a machine without udev by-id links). Reads the directory and resolves each entry; cost is a handful of file system calls. Tests: `bench_mcp_serial_guard_by_id_name` in `src/serial_guard.rs`.

### `check_supply` [@ANCHOR: bench_mcp_serial_guard_check_supply]

Decides whether a power supply may be opened at a given path.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The configured supply device path. |
| `reserved` | `&[String]` | The reserved list. |
| `r` | `&Roots` | The directories to read. |

Returns `Ok(())` to allow, or `Err(String)` with a message that tells the owner what to change. Refused when `reserved_reason` says the device is reserved (the message of that function), and refused when the path is bare, meaning not under the `by_id` directory and with a file name starting `ttyUSB` or `ttyACM`, and a `by_id` name exists for that device; the error then gives that stable path to use in the config. A bare path for a device that has no by-id name is allowed (a machine without those links must still work). A path under `by_id` is never refused for being bare. The supply is a power source for the bench: the guard exists so that a supply command is not sent to another instrument; it does not itself decide whether a supply may output (the supply tools have their own limits). No device is opened. Tests: `bench_mcp_serial_guard_check_supply` in `src/serial_guard.rs`.

### `check_reserved_open` [@ANCHOR: bench_mcp_serial_guard_check_reserved_open]

The reserved check alone, against the process's reserved list and the real file system, for any serial device.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The device a tool is about to open. |

Returns `Ok(())` when the device is not reserved (also when `set_reserved` has not been called, because the list is then empty), or `Err(String)` with the `reserved_reason` sentence. Called by `Serial::open` in `src/link.rs` first thing, before any open, so a reserved device is never touched by any driver. Reads the file system only. Tests: `bench_mcp_serial_guard_check_reserved_open` is cited from `src/config.rs`, which checks it with a `reserved_serial` entry read from a config.

## Scratch directories for tests

`testdir.rs` is compiled only for tests (`#[cfg(test)] mod testdir` in `main.rs`), and the integration tests under `tests/` include the same file with `#[path]`. It is the one way a test makes a scratch directory: a hand-built path under the temporary directory is never removed, and the temporary directory may be memory. `tools/check_test_temp_dirs.py` fails the lint run on a new `temp_dir().join(`. All names are made from the process id and a process-wide atomic counter, so two tests and two runs never share a directory.

### `TestDir` data structure and `TestDir::new` [@ANCHOR: bench_mcp_test_dir_new]

`TestDir` (a struct of one private field, `path: PathBuf`, deriving `Debug`) owns one fresh, empty directory and removes it, with all its contents, when the value is dropped. `TestDir::new` makes it.

| Argument | Type | Meaning |
|---|---|---|
| `tag` | `&str` | A label for the directory name, for people reading a leftover directory. Any text: each character that is not an ASCII letter, digit or `-` becomes `_`. |

Returns the `TestDir`. The directory is `<system temp dir>/bench_mcp_test_<pid>_<n>_<tag>`, where `<n>` is the next value of a shared counter. If that name already exists (left by an earlier process with the same id), the next counter value is tried until one is free. Panics, with the path and the OS error, if the directory cannot be created for any other reason (for example a missing or read-only temporary directory). Takes no lock; the atomic counter makes it safe to call from parallel tests. The invariants: the directory exists and is empty from `new` until drop and no other `TestDir` has the same path. Tests: `a_test_dir_is_fresh_unique_and_removed_with_its_contents_on_drop` cites it.

### `TestDir::path` [@ANCHOR: bench_mcp_test_dir_path]

Returns the directory as `&Path`, borrowed from the `TestDir` (so it cannot outlive it). No arguments, no errors, no side effects. Use `.path().join("file")` for files inside. To keep a path after the `TestDir` is gone, copy it with `to_path_buf()`, remembering that the directory is removed at drop. Tests: the same test as `TestDir::new`.

### `Deref` for `TestDir` [@ANCHOR: bench_mcp_test_dir_deref]

Lets a `TestDir` be used wherever a `&Path` is expected: `dir.join("x")`, `dir.is_dir()`, `&*dir`. The target type is `Path` and the returned reference is the same one as `path()`, borrowed from the value. No errors, no side effects. Tests: the same test as `TestDir::new` (`a.join(...)`, `a.is_dir()`).

### `AsRef<Path>` for `TestDir` [@ANCHOR: bench_mcp_test_dir_as_ref]

Lets a `TestDir` be passed to a function that takes `impl AsRef<Path>` (for example `std::fs::write(&dir, ...)` style calls or a helper with that bound) without calling `path()`. Returns the same `&Path` as `path()`. No errors, no side effects. Tests: the same test as `TestDir::new` (`let as_ref: &Path = a.as_ref()`).

### `Drop` for `TestDir` [@ANCHOR: bench_mcp_test_dir_drop]

Removes the directory and everything in it when the value goes out of scope: at the end of the test, and also while a failing test unwinds from a panic. First it makes the whole tree writable (`make_writable` sets mode 0700 on the directory and, recursively, on every subdirectory it can list), because a test may have made a subdirectory read-only to test a permission error and `remove_dir_all` would otherwise fail; then `remove_dir_all`, whose error is ignored (a directory that is already gone or cannot be removed is not reported, and a drop must never panic). Not run when the process is killed, aborts, calls `std::process::exit` or leaks the value with `mem::forget` (which `process_dir` does on purpose and removes at exit instead). Tests: `a_test_dir_is_fresh_unique_and_removed_with_its_contents_on_drop` and `a_panicking_test_still_removes_its_directory`.

### `process_dir` [@ANCHOR: bench_mcp_test_process_dir]

The output directory shared by every test in one test process that does not name its own: it is what `default_out_dir()` in `main.rs` returns under `cfg(test)` (the product code returns `<temp dir>/bench_mcp`, so a real run is unaffected), which is why no test run leaves files in the temporary directory.

It takes no arguments and returns `&'static Path`. The first call creates a `TestDir` tagged `process_out`, deliberately leaks it (`mem::forget`) so it is not dropped at the end of the first test, records its path in a `OnceLock` and registers a C `atexit` hook (`remove_process_dir`, which makes the tree writable and removes it). Later calls return the same path. The directory is therefore removed when the test process exits normally, whether or not a test failed; it is not removed if the process is killed by a signal or aborts (it would then remain in the temporary directory, named `bench_mcp_test_<pid>_<n>_process_out`). Thread safe (the `OnceLock` runs the creation once). The only `unsafe` is the `libc::atexit` call, whose hook touches only a `OnceLock` and the file system. Panics if the directory cannot be created (as `TestDir::new`). Tests: `the_process_directory_is_one_directory_that_exists_while_the_tests_run`; also `out_dir_for` in `main.rs`.

### `scratch` [@ANCHOR: bench_mcp_test_scratch]

Makes a fresh, empty directory inside `process_dir()` for a helper that has to return a path (a function returning `PathBuf`, a struct holding one), where a `TestDir` value cannot be handed around. Prefer `TestDir` wherever a value can own the directory.

| Argument | Type | Meaning |
|---|---|---|
| `tag` | `&str` | A label for the name; characters other than ASCII letters, digits and `-` become `_`. |

Returns the `PathBuf` `<process_dir>/<tag>_<n>`, where `<n>` comes from the same counter as `TestDir::new`, so two calls with one tag give two directories. The directory lives until the test process exits and is then removed with `process_dir`, whether or not a test failed. Panics, with the path and the OS error, when the directory cannot be created. Safe from parallel tests. Used by the tests of `scope`, `vna_tools`, `audio` and `serial_guard`. Tests: `the_process_directory_is_one_directory_that_exists_while_the_tests_run`.
