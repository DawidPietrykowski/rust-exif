use rexiv2::Metadata;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::{fs, io};

const ALLOWED_EXTENSIONS: [&str; 6] = ["heic", "jpg", "jpeg", "png", "arw", "dng"];

fn is_image(filename: &PathBuf) -> bool {
    if filename
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
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
    for allowed_extension in ALLOWED_EXTENSIONS {
        let lower_allowed = allowed_extension.to_lowercase();
        if lower_allowed == lower_passed {
            return true;
        }
    }
    false
}

fn visit_dirs(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    depth: i32,
    x: fn(&str) -> bool,
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
                if (depth != 0 || x(dir_name)) && !dir_name.starts_with(".") {
                    // filter
                    if depth == 0 {
                        println!("Passed {dir_name}");
                    }
                    visit_dirs(&path, paths, depth + 1, x)?;
                }
            } else {
                let path_buf = entry.path();
                if is_image(&path_buf) {
                    paths.push(path_buf);
                }
            }
        }
    }
    Ok(())
}

fn filter_string(string: &str, excluded_paths: Vec<String>) -> bool {
    for path in excluded_paths {
        if string.contains(path.as_str()) {
            return false;
        }
    }
    true
}

use clap::{Parser, Subcommand};

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
    dest: std::path::PathBuf,

    #[arg(short = 's', long)]
    src: std::path::PathBuf,
}

#[derive(Subcommand)]
enum FileCommand {
    Move,
    Copy,
    Delete,
}

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let command_name = match cli.command {
        FileCommand::Move => "Moving",
        FileCommand::Copy => "Copying",
        FileCommand::Delete => "Deleting",
    };

    let search_path = cli.src;
    let output_path: PathBuf = cli.dest;

    assert!(search_path.is_dir(), "Source path must be a directory");

    if output_path.exists() {
        assert!(output_path.is_dir(), "Output path must be a directory");
    } else {
        fs::create_dir(output_path.clone()).expect("Failed to create output directory");
    }

    let mut all_paths: Vec<PathBuf> = Vec::new();
    visit_dirs(search_path.as_ref(), &mut all_paths, 1, |_| true)
        .expect("Failed to iterate over directories");

    for path in all_paths {
        let relative_path = path
            .strip_prefix(search_path.clone())
            .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());
        let new_file_path = output_path.join(&relative_path);

        let res: Result<i32, String> = get_rating(path.clone());
        let Ok(rating) = res else{
            println!("Skipping {path:?} due to {}", res.err().unwrap_or("Unknown error".to_string()).to_string());
            continue;
        };

        let mut should_move = rating >= cli.threshold;
        if cli.inverse {
            should_move = !should_move;
        }

        if should_move {
            if cli.verbose {
                println!("Rated: {rating} {command_name} {path:?} {new_file_path:?}");
            }

            let dir_path = new_file_path.parent().unwrap();
            if !path_exists(dir_path.to_path_buf()) {
                fs::create_dir(dir_path.to_path_buf()).unwrap();
            }
            if !path_exists(new_file_path.clone()) {
                match cli.command {
                    FileCommand::Move => {
                        fs::rename(path, new_file_path).unwrap();
                    }
                    FileCommand::Copy => {
                        fs::copy(path, new_file_path).unwrap();
                    }
                    FileCommand::Delete => {
                        fs::remove_file(path).unwrap();
                    }
                }
            }
            // if !path.as_os_str().to_str().unwrap().contains("HEIC") {
            //     let jpg_suffix = match path.as_os_str().to_str().unwrap().contains(".JPG") {
            //         true => ".JPG",
            //         false => "-c.jpg",
            //     };
            //     let raw_path = (path
            //         .as_os_str()
            //         .to_str()
            //         .unwrap()
            //         .strip_suffix(jpg_suffix)
            //         .unwrap()
            //         .to_string()
            //         + ".ARW");
            //     let raw_path_str = raw_path.as_str();
            //     if path_exists(raw_path_str) {
            //         let out_path_raw =
            //             out_path.strip_suffix(jpg_suffix).unwrap().to_string() + ".ARW";
            //         fs::copy(raw_path_str, Path::new(out_path_raw.as_str())).unwrap();
            //     }
            // }
        }
    }
}

pub fn path_exists(path: PathBuf) -> bool {
    fs::metadata(path).is_ok()
}

fn get_rating(filename: PathBuf) -> Result<i32, String> {
    if !path_exists(filename.clone()) {
        return Err("File doesn't exist".to_string());
    }

    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
            Ok(rating)
        }
        Err(e) => Err(e.to_string()),
    }
}
