#![forbid(unsafe_code)]

//! Command-line adapter over FontSpace's typed operations (spec/13). A thin front
//! end: it parses arguments (see [`parse`]), loads/saves canonical JSON with atomic
//! writes, and invokes the same `fontspace-ops` / `fontspace-render` functions the
//! GUI and MCP use. It never reimplements domain logic.

mod parse;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use fontspace_export::{
    ExportError, column_scan_config, encode_raw_binary, generate_image, row_scan_config,
    validate_export,
};
use fontspace_json::{
    JsonError, load as load_json, load_fragment, save as save_json, write_fragment,
};
use fontspace_model::{
    ExportConfig, FontSpace, FontSpaceFragment, GlyphSet, GlyphSetId, IdGen, Limits, PageId,
    RandomIdGen, SequentialIdGen,
};
use fontspace_ops::{
    AddExportConfig, ChangeSet, ExtractGlyphs, FontSpaceError, GlyphRef, GlyphSelector,
    PasteGlyphs, PixelEdit, SetPixels, ShiftGlyphs, add_export_config, extract_glyphs,
    paste_glyphs, resolve_pages_in, set_pixels, shift_glyphs,
};
use fontspace_render::{TextGridRequest, TextStringRequest, render_text_grid, render_text_string};

use parse::{
    ParseError, RenderSubject, parse_code_token, parse_glyph_mapping, parse_glyph_selector,
    parse_layout, parse_overflow, parse_page_selector, parse_pixel, parse_size_conversion,
    resolve_glyph_set, resolve_render_subject,
};

#[derive(Parser)]
#[command(
    name = "fontspace",
    about = "FontSpace CLI — edit and render .fontspace.json documents (spec/13)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Use the deterministic sequential id generator (for reproducible fixtures).
    #[arg(long, global = true)]
    seq: bool,

    /// For mutating commands: print the resulting change-set summary, write nothing.
    #[arg(long, global = true)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new, empty document.
    New {
        path: PathBuf,
        #[arg(long, default_value = "")]
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    /// Print a summary of a document.
    Info { path: PathBuf },
    /// Set pixels on one glyph (one undo entry).
    SetPixels {
        path: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long)]
        page: String,
        #[arg(long)]
        code: String,
        /// A pixel edit `X,Y,VALUE` (VALUE = 0/1 or on/off). Repeatable.
        #[arg(long = "pixel", required = true)]
        pixels: Vec<String>,
    },
    /// Shift selected glyphs by (dx, dy).
    Shift {
        path: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long, default_value = "all")]
        pages: String,
        #[arg(long)]
        glyphs: String,
        #[arg(long, allow_hyphen_values = true)]
        dx: i16,
        #[arg(long, allow_hyphen_values = true)]
        dy: i16,
        #[arg(long, default_value = "discard")]
        overflow: String,
    },
    /// Render glyphs as a text grid to stdout. The render subject is one of
    /// `--glyphs` (a code selector; defaults to *all* glyphs), `--text` (an input
    /// string rendered as one line), or `--text-nl` (like `--text`, but a newline
    /// starts a new line). At most one may be given. For `--text`/`--text-nl`,
    /// each input character maps to a `code` by its Unicode scalar (the byte value
    /// for Latin-1 input) and characters with no character-set entry are ignored.
    RenderText {
        path: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long, default_value = "all")]
        pages: String,
        /// A glyph code selector (e.g. `A-Z`, `0x20-0x7E`). Defaults to all glyphs.
        #[arg(long)]
        glyphs: Option<String>,
        /// Render this string as one line of text.
        #[arg(long)]
        text: Option<String>,
        /// Render this string, starting a new line at each newline codepoint.
        #[arg(long = "text-nl")]
        text_nl: Option<String>,
        #[arg(long, default_value = "#")]
        on: String,
        #[arg(long, default_value = ".")]
        off: String,
        #[arg(long = "glyph-sep", default_value = "")]
        glyph_separator: String,
        #[arg(long, default_value = "glyphs-horizontal")]
        layout: String,
        #[arg(long, default_value_t = 1)]
        scale_x: usize,
        #[arg(long, default_value_t = 1)]
        scale_y: usize,
    },
    /// Extract glyphs from one page into a fragment JSON file (spec/08 §8.5).
    Extract {
        path: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long)]
        page: String,
        /// A glyph code selector (e.g. `A-Z`, `0x20-0x7E`). Defaults to all glyphs.
        #[arg(long)]
        glyphs: Option<String>,
        /// Destination fragment JSON file.
        #[arg(long)]
        output: PathBuf,
    },
    /// Paste a glyph fragment onto one page (spec/08 §8.3). Geometry must match
    /// exactly (`RequireExact`); no destination code is guessed.
    Paste {
        path: PathBuf,
        /// The fragment JSON file to paste (from `extract`).
        #[arg(long)]
        fragment: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long)]
        page: String,
        /// `by-code` (default), `by-slot`, or `sequential-from-code:CODE`
        /// (spec/08 §8.3).
        #[arg(long, default_value = "by-code")]
        mapping: String,
        /// `require-exact` (default), `center`, `scale-nearest`, or `place-at:X,Y`
        /// (spec/08 §8.3).
        #[arg(long, default_value = "require-exact")]
        size: String,
    },
    /// Add a standard row/column-scan ROM export config to the document (spec/10).
    AddExportConfig {
        path: PathBuf,
        /// A name for the new export config.
        #[arg(long)]
        name: String,
        #[arg(long)]
        glyph_set: String,
        /// Comma-separated page names in ROM page order, or `all` (spec/10 §10.4).
        #[arg(long, default_value = "all")]
        pages: String,
        /// Address-space width for the code dimension: the ROM spans `2^code_bits`
        /// codes (spec/10 §10.2). 7 → the 128-code ASCII range.
        #[arg(long, default_value_t = 7)]
        code_bits: u8,
        /// `row` (the row is addressed, columns on the data bits — the usual text ROM)
        /// or `column` (spec/10 §10.4).
        #[arg(long, default_value = "row")]
        scan: String,
    },
    /// Validate that an export config is a strict 1:1 ROM and print its shape (spec/10
    /// §10.7).
    ValidateExport {
        path: PathBuf,
        /// The export config's name.
        #[arg(long)]
        config: String,
    },
    /// Render an export config to a raw binary ROM image (spec/10 §10.9). Validates
    /// first; `--dry-run` prints the summary and writes nothing.
    Export {
        path: PathBuf,
        #[arg(long)]
        config: String,
        /// Destination `.bin` file. Required unless `--dry-run` (which writes nothing).
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

/// Everything a command can fail with; rendered to stderr by `main`.
#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Op(#[from] FontSpaceError),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("reading {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("writing {path}: {source}")]
    WriteFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} already exists (refusing to overwrite; delete it first)")]
    FileExists(String),
    #[error("--page must match exactly one page, but matched {count}")]
    PageTargetNotUnique { count: usize },
    #[error(transparent)]
    Export(#[from] ExportError),
    #[error("no export config named {0:?}")]
    ExportConfigNotFound(String),
    #[error("export config {config:?} references a glyph set not in this document")]
    ExportSourceMissing { config: String },
    #[error("no page named {0:?} in the glyph set")]
    PageNotFound(String),
    #[error("unknown scan {0:?} (expected 'row' or 'column')")]
    UnknownScan(String),
    #[error("--output is required to write the ROM image (omit it only with --dry-run)")]
    OutputRequired,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    let mut ids: Box<dyn IdGen> = if cli.seq {
        Box::new(SequentialIdGen::new())
    } else {
        Box::new(RandomIdGen)
    };

    match cli.command {
        Command::New {
            path,
            name,
            description,
        } => {
            let doc = FontSpace::new(ids.as_mut(), name, description);
            if cli.dry_run {
                print!("{}", save_json(&doc));
            } else {
                // Never clobber an existing document (spec/16 §16.2 — user files
                // are never corrupted).
                if path.exists() {
                    return Err(CliError::FileExists(path.display().to_string()));
                }
                write_document(&path, &doc)?;
                println!("created {}", path.display());
            }
            Ok(())
        }

        Command::Info { path } => {
            let doc = load_document(&path)?;
            print_info(&doc);
            Ok(())
        }

        Command::SetPixels {
            path,
            glyph_set,
            page,
            code,
            pixels,
        } => {
            let mut doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            // set-pixels targets one glyph on one page.
            let page_id = resolve_single_page(&doc, glyph_set_id, &page)?;
            let code = parse_code_token(&code)?;
            let edits = pixels
                .iter()
                .map(|token| parse_pixel(token))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|p| PixelEdit {
                    x: p.x,
                    y: p.y,
                    value: p.value,
                })
                .collect();
            let change_set = set_pixels(
                &mut doc,
                &SetPixels {
                    target: GlyphRef {
                        glyph_set_id,
                        page_id,
                        code,
                    },
                    edits,
                },
            )?;
            finish_mutation(&path, &doc, &change_set, cli.dry_run)
        }

        Command::Shift {
            path,
            glyph_set,
            pages,
            glyphs,
            dx,
            dy,
            overflow,
        } => {
            let mut doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            let change_set = shift_glyphs(
                &mut doc,
                &ShiftGlyphs {
                    glyph_set_id,
                    pages: parse_page_selector(&pages),
                    glyphs: parse_glyph_selector(&glyphs)?,
                    dx,
                    dy,
                    overflow: parse_overflow(&overflow)?,
                },
            )?;
            finish_mutation(&path, &doc, &change_set, cli.dry_run)
        }

        Command::RenderText {
            path,
            glyph_set,
            pages,
            glyphs,
            text,
            text_nl,
            on,
            off,
            glyph_separator,
            layout,
            scale_x,
            scale_y,
        } => {
            let doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            let pages = parse_page_selector(&pages);
            let subject =
                resolve_render_subject(glyphs.as_deref(), text.as_deref(), text_nl.as_deref())?;
            let output = match subject {
                RenderSubject::Glyphs(glyphs) => render_text_grid(
                    &doc,
                    &TextGridRequest {
                        glyph_set_id,
                        pages,
                        glyphs,
                        on,
                        off,
                        glyph_separator,
                        row_separator: "\n".to_string(),
                        page_separator: "\n\n".to_string(),
                        layout: parse_layout(&layout)?,
                        scale_x,
                        scale_y,
                    },
                )?,
                // --text / --text-nl: an ordered run of characters; `layout` does not
                // apply (text is always a left-to-right, top-to-bottom run).
                RenderSubject::Text(rows) => render_text_string(
                    &doc,
                    &TextStringRequest {
                        glyph_set_id,
                        pages,
                        rows,
                        on,
                        off,
                        glyph_separator,
                        row_separator: "\n".to_string(),
                        page_separator: "\n\n".to_string(),
                        scale_x,
                        scale_y,
                    },
                )?,
            };
            println!("{output}");
            Ok(())
        }

        Command::Extract {
            path,
            glyph_set,
            page,
            glyphs,
            output,
        } => {
            let doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            // Extract targets one page.
            let page_id = resolve_single_page(&doc, glyph_set_id, &page)?;
            let glyphs = match glyphs {
                Some(spec) => parse_glyph_selector(&spec)?,
                None => GlyphSelector::All,
            };
            let fragment = extract_glyphs(
                &doc,
                &ExtractGlyphs {
                    glyph_set_id,
                    page_id,
                    glyphs,
                },
            )?;
            let count = fragment.glyphs.len();
            let fragment = FontSpaceFragment::Glyphs(fragment);
            if cli.dry_run {
                println!("dry-run: {count} glyph(s) extracted — nothing written");
            } else {
                write_fragment(&output, &fragment)?;
                println!("{count} glyph(s) written to {}", output.display());
            }
            Ok(())
        }

        Command::Paste {
            path,
            fragment,
            glyph_set,
            page,
            mapping,
            size,
        } => {
            let mut doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            let page_id = resolve_single_page(&doc, glyph_set_id, &page)?;
            let FontSpaceFragment::Glyphs(fragment) = load_fragment_file(&fragment)?;
            let change_set = paste_glyphs(
                &mut doc,
                &PasteGlyphs {
                    fragment,
                    target_glyph_set_id: glyph_set_id,
                    target_page_id: page_id,
                    mapping: parse_glyph_mapping(&mapping)?,
                    size_conversion: parse_size_conversion(&size)?,
                },
            )?;
            finish_mutation(&path, &doc, &change_set, cli.dry_run)
        }

        Command::AddExportConfig {
            path,
            name,
            glyph_set,
            pages,
            code_bits,
            scan,
        } => {
            let mut doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            let config = {
                let gs = doc
                    .glyph_set(glyph_set_id)
                    .ok_or_else(|| ParseError::GlyphSetNotFound(glyph_set.clone()))?;
                let page_ids = pages_of(gs, &pages)?;
                match scan.as_str() {
                    "row" => row_scan_config(ids.as_mut(), &name, gs, page_ids, code_bits),
                    "column" => column_scan_config(ids.as_mut(), &name, gs, page_ids, code_bits),
                    other => return Err(CliError::UnknownScan(other.to_string())),
                }
            };
            // Insert through the domain op — the same invertible path the GUI uses —
            // rather than pushing onto the vector directly (spec/07 §7.2).
            add_export_config(&mut doc, &AddExportConfig { config })?;
            if cli.dry_run {
                println!("dry-run: export config {name:?} not written");
            } else {
                write_document(&path, &doc)?;
                println!("added export config {name:?}");
            }
            Ok(())
        }

        Command::ValidateExport { path, config } => {
            let doc = load_document(&path)?;
            let export_config = find_export_config(&doc, &config)?;
            let glyph_set = source_glyph_set(&doc, export_config, &config)?;
            let summary = validate_export(glyph_set, export_config, &Limits::default())?;
            println!("{summary}");
            Ok(())
        }

        Command::Export {
            path,
            config,
            output,
        } => {
            let doc = load_document(&path)?;
            let export_config = find_export_config(&doc, &config)?;
            let glyph_set = source_glyph_set(&doc, export_config, &config)?;
            let limits = Limits::default();
            // Always validate first: never emit bytes from a non-1:1 config.
            let summary = validate_export(glyph_set, export_config, &limits)?;
            if cli.dry_run {
                println!(
                    "dry-run: valid — {} output bytes (nothing written)",
                    summary.output_bytes
                );
                println!("{summary}");
            } else {
                // `--output` is only needed when actually writing, so it is required
                // here rather than by clap — a dry run validates without one.
                let output = output.ok_or(CliError::OutputRequired)?;
                let image = generate_image(glyph_set, export_config, &limits)?;
                let bytes = encode_raw_binary(&image, summary.data_bits);
                fs::write(&output, &bytes).map_err(|source| CliError::WriteFile {
                    path: output.display().to_string(),
                    source,
                })?;
                println!("wrote {} bytes to {}", bytes.len(), output.display());
            }
            Ok(())
        }
    }
}

/// The ordered page ids named by `pages` (comma-separated names, or `all`), for an
/// export's page sequence (spec/10 §10.4).
fn pages_of(glyph_set: &GlyphSet, pages: &str) -> Result<Vec<PageId>, CliError> {
    if pages.trim() == "all" {
        return Ok(glyph_set.pages.iter().map(|page| page.id).collect());
    }
    pages
        .split(',')
        .map(|name| {
            let name = name.trim();
            glyph_set
                .pages
                .iter()
                .find(|page| page.name == name)
                .map(|page| page.id)
                .ok_or_else(|| CliError::PageNotFound(name.to_string()))
        })
        .collect()
}

/// The export config named `name`, or a not-found error.
fn find_export_config<'a>(doc: &'a FontSpace, name: &str) -> Result<&'a ExportConfig, CliError> {
    doc.export_configs
        .iter()
        .find(|config| config.name == name)
        .ok_or_else(|| CliError::ExportConfigNotFound(name.to_string()))
}

/// The glyph set an export config sources from (spec/10 §10.1), or a missing-source error.
fn source_glyph_set<'a>(
    doc: &'a FontSpace,
    config: &ExportConfig,
    config_name: &str,
) -> Result<&'a GlyphSet, CliError> {
    doc.glyph_set(config.source.glyph_set_id)
        .ok_or_else(|| CliError::ExportSourceMissing {
            config: config_name.to_string(),
        })
}

fn print_info(doc: &FontSpace) {
    println!("format_version: {}", doc.format_version);
    println!("name: {:?}", doc.metadata.name);
    println!("character_sets: {}", doc.character_sets.len());
    for character_set in &doc.character_sets {
        println!(
            "  {:?} — {} entries",
            character_set.name,
            character_set.entries.len()
        );
    }
    println!("glyph_sets: {}", doc.glyph_sets.len());
    for glyph_set in &doc.glyph_sets {
        let glyphs: usize = glyph_set.pages.iter().map(|page| page.glyphs.len()).sum();
        println!(
            "  {:?} — {}×{}, {} page(s), {} stored glyph(s)",
            glyph_set.name,
            glyph_set.glyph_size.width,
            glyph_set.glyph_size.height,
            glyph_set.pages.len(),
            glyphs
        );
    }
}

/// Reports the outcome of a mutating command, writing the document unless `--dry-run`.
fn finish_mutation(
    path: &Path,
    doc: &FontSpace,
    change_set: &ChangeSet,
    dry_run: bool,
) -> Result<(), CliError> {
    if change_set.is_empty() {
        println!("no changes");
        return Ok(());
    }
    let summary = format!(
        "{} change(s), {} warning(s)",
        change_set.object_changes.len(),
        change_set.warnings.len()
    );
    for warning in &change_set.warnings {
        eprintln!("warning: {warning:?}");
    }
    if dry_run {
        println!("dry-run: {summary} — nothing written");
    } else {
        write_document(path, doc)?;
        println!("{summary} written to {}", path.display());
    }
    Ok(())
}

fn load_document(path: &Path) -> Result<FontSpace, CliError> {
    let text = read_to_string_in_context(path)?;
    let outcome = load_json(&text)?;
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(outcome.document)
}

fn load_fragment_file(path: &Path) -> Result<FontSpaceFragment, CliError> {
    let text = read_to_string_in_context(path)?;
    Ok(load_fragment(&text)?)
}

/// Reads a file to a string, naming the path on failure so a missing/unreadable file
/// reports *which* file was at fault (CLAUDE.md: errors identify object context).
fn read_to_string_in_context(path: &Path) -> Result<String, CliError> {
    fs::read_to_string(path).map_err(|source| CliError::ReadFile {
        path: path.display().to_string(),
        source,
    })
}

/// Resolves `--page` to exactly one page id, rejecting an ambiguous or empty match
/// rather than silently narrowing a list (CLAUDE.md "no hidden remapping"). Shared by
/// the single-page commands (`set-pixels`, `extract`, `paste`).
fn resolve_single_page(
    doc: &FontSpace,
    glyph_set_id: GlyphSetId,
    page: &str,
) -> Result<PageId, CliError> {
    let resolved = resolve_pages_in(doc, glyph_set_id, &parse_page_selector(page))?;
    match resolved.as_slice() {
        [only] => Ok(*only),
        other => Err(CliError::PageTargetNotUnique { count: other.len() }),
    }
}

/// Writes canonical JSON atomically (write `.tmp`, flush, rename over the
/// destination). Delegates to [`fontspace_json::write_document`] — the single home
/// for the atomic-replacement guarantee (spec/16 §16.2).
fn write_document(path: &Path, doc: &FontSpace) -> Result<(), CliError> {
    fontspace_json::write_document(path, doc)?;
    Ok(())
}
