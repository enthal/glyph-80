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

use fontspace_json::{JsonError, load as load_json, save as save_json};
use fontspace_model::{FontSpace, IdGen, RandomIdGen, SequentialIdGen};
use fontspace_ops::{
    ChangeSet, FontSpaceError, GlyphRef, PixelEdit, SetPixels, ShiftGlyphs, resolve_pages_in,
    set_pixels, shift_glyphs,
};
use fontspace_render::{TextGridRequest, render_text_grid};

use parse::{
    ParseError, parse_code_token, parse_glyph_selector, parse_layout, parse_overflow,
    parse_page_selector, parse_pixel, resolve_glyph_set,
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
    /// Render selected glyphs as a text grid to stdout.
    RenderText {
        path: PathBuf,
        #[arg(long)]
        glyph_set: String,
        #[arg(long, default_value = "all")]
        pages: String,
        #[arg(long)]
        glyphs: String,
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
            // Resolve the page by name through ops (rejects ambiguity precisely).
            let page_id = resolve_pages_in(&doc, glyph_set_id, &parse_page_selector(&page))?
                .into_iter()
                .next()
                .ok_or(FontSpaceError::PageNameNotFound {
                    glyph_set: glyph_set_id,
                    name: page.clone(),
                })?;
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
            on,
            off,
            glyph_separator,
            layout,
            scale_x,
            scale_y,
        } => {
            let doc = load_document(&path)?;
            let glyph_set_id = resolve_glyph_set(&doc, &glyph_set)?;
            let output = render_text_grid(
                &doc,
                &TextGridRequest {
                    glyph_set_id,
                    pages: parse_page_selector(&pages),
                    glyphs: parse_glyph_selector(&glyphs)?,
                    on,
                    off,
                    glyph_separator,
                    row_separator: "\n".to_string(),
                    page_separator: "\n\n".to_string(),
                    layout: parse_layout(&layout)?,
                    scale_x,
                    scale_y,
                },
            )?;
            println!("{output}");
            Ok(())
        }
    }
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
    let text = fs::read_to_string(path)?;
    let outcome = load_json(&text)?;
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    Ok(outcome.document)
}

/// Writes canonical JSON atomically: write a sibling `.tmp`, then rename over the
/// destination — a failed write never corrupts the original (spec/16 §16.2).
fn write_document(path: &Path, doc: &FontSpace) -> Result<(), CliError> {
    let json = save_json(doc);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, json.as_bytes())?;
    fs::rename(&tmp, path)?;
    Ok(())
}
