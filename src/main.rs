use crate::xmp::read_rating_xmp;
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rexiv2::Metadata;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

mod xmp;

const IMAGE_EXTENSIONS: [&str; 4] = ["heic", "jpg", "jpeg", "png"];
// TODO: restore multiple RAW file extension support
// const RAW_IMAGE_EXENSIONS: [&str; 2] = ["arw", "dng"];
const VIDEOS_EXTENSIONS: [&str; 3] = ["mov", "mp4", "avi"];

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: FileCommand,

    #[arg(short = 't', long, default_value_t = 5)]
    threshold: i32,

    #[arg(short = 'i', long, default_value_t = false)]
    inverse: bool,

    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,

    #[arg(short = 'd', long)]
    dest: Option<std::path::PathBuf>,

    #[arg(short = 's', long)]
    src: std::path::PathBuf,

    #[arg(short = 'e', long)]
    exclude: Vec<String>,

    #[arg(short = 'f', long, default_value_t = false)]
    flip_exclusion: bool,

    #[arg(short = 'm', long, default_value_t = false)]
    match_raws: bool,

    #[arg(short = 'a', long, default_value_t = false)]
    include_videos: bool,

    #[arg(short = 'l', long)]
    label: Option<String>,

    #[arg(short = 'n', long, default_value_t = false)]
    dry_run: bool,

    #[arg(short = 'c', long, default_value_t = ComparisonCommand::MoreEqual)]
    comparison_command: ComparisonCommand,
}

#[derive(Subcommand, PartialEq)]
enum FileCommand {
    Move,
    Copy,
    Delete,
    Print,
    DeleteRaws,
    CopyRaws,
}

impl Display for ComparisonCommand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ComparisonCommand::MoreEqual => write!(f, "more-equal"),
            ComparisonCommand::LessEqual => write!(f, "less-equal"),
            ComparisonCommand::Equal => write!(f, "equal"),
        }
    }
}

#[derive(ValueEnum, Clone, Debug)]
enum ComparisonCommand {
    MoreEqual,
    LessEqual,
    Equal,
}

#[derive(Clone, Eq, PartialEq, Debug)]
struct Entry {
    path: PathBuf,
    raw_path: Option<PathBuf>,
}

impl Entry {
    fn new(path: PathBuf) -> Entry {
        Entry {
            path,
            raw_path: None,
        }
    }

    fn new_with_raw(path: PathBuf, raw_path: PathBuf) -> Entry {
        Entry {
            path,
            raw_path: Some(raw_path),
        }
    }
}

impl Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(" p: {:?}", self.path))?;
        if let Some(raw_path) = &self.raw_path {
            f.write_fmt(format_args!(" r: {:?}", raw_path))?;
        }
        Ok(())
    }
}

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let command_name = match cli.command {
        FileCommand::Move => "Moving",
        FileCommand::Copy => "Copying",
        FileCommand::Delete => "Deleting",
        FileCommand::Print => "Printing",
        FileCommand::DeleteRaws => "Deleting raw file",
        FileCommand::CopyRaws => "Copying raw file",
    };

    let search_path = cli.src;

    assert!(search_path.is_dir(), "Source path must be a directory");

    let output_path: Option<PathBuf> = cli.dest;

    let requires_destination = cli.command == FileCommand::Move
        || cli.command == FileCommand::Copy
        || cli.command == FileCommand::CopyRaws;

    if requires_destination {
        assert!(output_path.is_some(), "Destination path must be specified");
        if output_path.clone().unwrap().exists() {
            assert!(
                output_path.clone().unwrap().is_dir(),
                "Output path must be a directory"
            );
        } else {
            fs::create_dir(output_path.clone().unwrap().clone())
                .expect("Failed to create output directory");
        }
    }

    let mut all_paths: Vec<Entry> = Vec::new();
    visit_dirs(
        search_path.as_ref(),
        &mut all_paths,
        0,
        cli.exclude,
        cli.flip_exclusion,
        cli.include_videos,
        cli.match_raws,
        cli.verbose,
    )
    .expect("Failed to iterate over directories");

    for path in all_paths {
        let relative_path = path
            .path
            .strip_prefix(search_path.clone())
            .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());

        let res: Result<i32> = get_rating(path.path.clone());
        let Ok(rating) = res else {
            eprintln!(
                "Skipping {path:?} due to {}",
                res.err().unwrap_or(anyhow!("Unknown error")).to_string()
            );
            continue;
        };

        let pass_label_check = if let Some(ref label) = cli.label {
            let res: Result<Option<String>, String> = get_label(path.path.clone());
            let Ok(label_res) = res else {
                eprintln!(
                    "Skipping {path:?} due to {}",
                    res.err().unwrap_or("Unknown error".to_string()).to_string()
                );
                continue;
            };
            match label_res {
                Some(label_res) => label_res == *label,
                None => false,
            }
        } else {
            true
        };

        let pass_treshold_check = match cli.comparison_command {
            ComparisonCommand::MoreEqual => rating >= cli.threshold,
            ComparisonCommand::LessEqual => rating <= cli.threshold,
            ComparisonCommand::Equal => rating == cli.threshold,
        };

        let mut should_move = pass_treshold_check && pass_label_check;

        if cli.inverse {
            should_move = !should_move;
        }

        if should_move {
            if cli.verbose {
                eprintln!("Rated: {rating} {command_name} {path}");
            }

            let mut dest_dir: Option<PathBuf> = None;
            if requires_destination {
                let Some(output_path) = output_path.clone() else {
                    panic!("Did not specify destination path");
                };
                let new_file_path = output_path.join(&relative_path);
                let dir_path: &Path = new_file_path.parent().unwrap();
                if !path_exists(dir_path.to_path_buf()) {
                    eprintln!("Creating destination directory: {dir_path:?}");
                    fs::create_dir(dir_path.to_path_buf()).unwrap();
                }
                dest_dir = Some(dir_path.to_path_buf());
            }

            apply_command(
                &cli.command,
                cli.verbose,
                path.clone(),
                dest_dir,
                cli.dry_run,
            );
        }
    }
}

fn apply_command(
    command: &FileCommand,
    verbose: bool,
    path: Entry,
    destination_directory: Option<PathBuf>,
    dry_run: bool,
) {
    match command {
        FileCommand::Move => {
            let new_file_path = destination_directory
                .clone()
                .unwrap()
                .join(path.path.file_name().unwrap());
            move_file(path.path, new_file_path, dry_run, verbose);
            if let Some(raw_path) = path.raw_path {
                let new_file_path = destination_directory
                    .unwrap()
                    .join(raw_path.file_name().unwrap());
                move_file(raw_path, new_file_path, dry_run, verbose);
            }
        }
        FileCommand::Copy => {
            let new_file_path = destination_directory
                .clone()
                .unwrap()
                .join(path.path.file_name().unwrap());
            copy_file(path.path, new_file_path, dry_run, verbose);
            if let Some(raw_path) = path.raw_path {
                let new_file_path = destination_directory
                    .unwrap()
                    .join(raw_path.file_name().unwrap());
                copy_file(raw_path, new_file_path, dry_run, verbose);
            }
        }
        FileCommand::Delete => {
            remove_file(path.path, dry_run, verbose);
            if let Some(raw_path) = path.raw_path {
                remove_file(raw_path, dry_run, verbose);
            }
        }
        FileCommand::Print => {
            println!("{}", path.path.as_os_str().to_str().unwrap());
            if let Some(raw_path) = path.raw_path {
                println!("{}", raw_path.as_os_str().to_str().unwrap());
            }
        }
        FileCommand::DeleteRaws => {
            if let Some(raw_path) = path.raw_path {
                remove_file(raw_path, dry_run, verbose);
            }
        }
        FileCommand::CopyRaws => {
            if let Some(raw_path) = path.raw_path {
                let new_file_path = destination_directory
                    .unwrap()
                    .join(raw_path.file_name().unwrap());
                copy_file(raw_path, new_file_path, dry_run, verbose);
            }
        }
    }
}

fn remove_file<P: AsRef<Path>>(path: P, dry_run: bool, verbose: bool) {
    if verbose {
        eprintln!("rm {:?}", path.as_ref());
    }
    match dry_run {
        true => println!("rm {:?}", path.as_ref()),
        false => fs::remove_file(path).unwrap(),
    }
}

fn move_file<P: AsRef<Path>>(path: P, dest: P, dry_run: bool, verbose: bool) {
    if dest.as_ref().exists() {
        if verbose {
            eprintln!(
                "Skipping {:?} as {:?} it already exists",
                path.as_ref(),
                dest.as_ref()
            );
        }
        return;
    }
    if verbose {
        eprintln!("mv {:?} {:?}", path.as_ref(), dest.as_ref());
    }
    match dry_run {
        true => println!("mv {:?} {:?}", path.as_ref(), dest.as_ref()),
        false => fs::rename(path, dest).unwrap(),
    }
}

fn copy_file<P: AsRef<Path>>(path: P, dest: P, dry_run: bool, verbose: bool) {
    if dest.as_ref().exists() {
        if verbose {
            eprintln!(
                "Skipping {:?} as {:?} it already exists",
                path.as_ref(),
                dest.as_ref()
            );
        }
        return;
    }
    if verbose {
        eprintln!("cp {:?} {:?}", path.as_ref(), dest.as_ref());
    }
    match dry_run {
        true => {
            println!("cp {:?} {:?}", path.as_ref(), dest.as_ref());
        }
        false => {
            fs::copy(path, dest).unwrap();
        }
    }
}

fn visit_dirs(
    dir: &Path,
    paths: &mut Vec<Entry>,
    depth: i32,
    excluded_paths: Vec<String>,
    flip_exclusion: bool,
    include_videos: bool,
    raws_matched: bool,
    verbose: bool,
) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path
                    .as_path()
                    .file_name()
                    .expect("Could not get relative path")
                    .to_str()
                    .unwrap();
                let mut filter_res = filter_string(dir_name, excluded_paths.clone());
                if flip_exclusion {
                    filter_res = !filter_res;
                }
                if (depth != 0 || filter_res) && !dir_name.starts_with(".") {
                    // filter
                    if verbose && depth == 0 {
                        eprintln!("Including {dir_name}");
                    }
                    visit_dirs(
                        &path,
                        paths,
                        depth + 1,
                        excluded_paths.clone(),
                        flip_exclusion,
                        include_videos,
                        raws_matched,
                        verbose,
                    )?;
                }
            } else {
                let path_buf = entry.path();
                if is_file_allowed(&path_buf, include_videos) {
                    let raw_file_path = path_buf.with_extension("ARW");
                    if raws_matched && raw_file_path.exists() {
                        if verbose {
                            eprintln!("Matched raw file {raw_file_path:?}");
                        }
                        paths.push(Entry::new_with_raw(path_buf, raw_file_path));
                    } else {
                        paths.push(Entry::new(path_buf));
                    }
                } else {
                    if verbose {
                        eprintln!("Skipping file {path_buf:?}");
                    }
                }
            }
        }
    }
    Ok(())
}

fn filter_string(string: &str, excluded_paths: Vec<String>) -> bool {
    for path in excluded_paths {
        if string.contains(&path) {
            return false;
        }
    }
    true
}

fn path_exists(path: PathBuf) -> bool {
    fs::metadata(path).is_ok()
}

fn get_rating(filename: PathBuf) -> Result<i32> {
    if !path_exists(filename.clone()) {
        anyhow::bail!("File doesn't exist");
    }

    // Use xmp-toolkit for video files
    if is_video(&filename) {
        return Ok(read_rating_xmp(filename.clone()).unwrap_or(0));
    }

    // Use rexiv2 for image files
    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
            Ok(rating)
        }
        Err(e) => anyhow::bail!(e),
    }
}

fn is_video(path: &Path) -> bool {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or("")
        .to_lowercase();
    VIDEOS_EXTENSIONS.contains(&extension.as_str())
}

fn get_label(filename: PathBuf) -> Result<Option<String>, String> {
    if !path_exists(filename.clone()) {
        return Err("File doesn't exist".to_string());
    }

    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            let label = meta.get_tag_string("Xmp.xmp.Label");
            match label {
                Ok(label) => Ok(Some(label)),
                Err(_) => Ok(None),
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn is_file_allowed(filename: &PathBuf, include_videos: bool) -> bool {
    if filename
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(".")
    {
        return false;
    }

    let ext = filename
        .extension()
        .unwrap_or(OsStr::new(""))
        .to_str()
        .unwrap();
    let lower_passed = ext.to_lowercase();

    let mut ext: Vec<&str> = IMAGE_EXTENSIONS.to_vec();

    if include_videos {
        ext.extend(VIDEOS_EXTENSIONS.iter());
    }

    for allowed_extension in ext {
        let lower_allowed = allowed_extension.to_lowercase();
        if lower_allowed == lower_passed {
            return true;
        }
    }
    false
}
