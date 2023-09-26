use clap::{Parser, Subcommand};
use rexiv2::Metadata;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::{fs, io};

const ALLOWED_EXTENSIONS: [&str; 6] = ["heic", "jpg", "jpeg", "png", "arw", "dng"];

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

    #[arg(short = 'e', long)]
    exclude: Vec<String>,

    #[arg(short = 'f', long, default_value_t = false)]
    flip_exclusion: bool,
}

#[derive(Subcommand)]
enum FileCommand {
    Move,
    Copy,
    Delete,
    Print,
}

fn main() {
    let cli: Cli = Cli::parse();

    rexiv2::initialize().expect("Unable to initialize rexiv2");

    let command_name = match cli.command {
        FileCommand::Move => "Moving",
        FileCommand::Copy => "Copying",
        FileCommand::Delete => "Deleting",
        FileCommand::Print => "Printing",
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
    visit_dirs(
        search_path.as_ref(),
        &mut all_paths,
        0,
        cli.exclude,
        cli.flip_exclusion,
    )
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
                    FileCommand::Print => {
                        let path_str = path.as_os_str().to_str().unwrap();
                        println!("{path_str}");
                    }
                }
            } else if cli.verbose {
                println!("Skipping {new_file_path:?} as it already exists");
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

fn visit_dirs(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    depth: i32,
    excluded_paths: Vec<String>,
    flip_exclusion: bool,
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
                    if depth == 0 {
                        println!("Including {dir_name}");
                    }
                    visit_dirs(
                        &path,
                        paths,
                        depth + 1,
                        excluded_paths.clone(),
                        flip_exclusion,
                    )?;
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
        if string.contains(&path) {
            return false;
        }
    }
    true
}

fn path_exists(path: PathBuf) -> bool {
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

fn is_image(filename: &PathBuf) -> bool {
    if filename.starts_with(".") {
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
