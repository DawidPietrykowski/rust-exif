use clap::{Parser, Subcommand, ValueEnum};
use rexiv2::Metadata;
use std::ffi::OsStr;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Iter, Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};
use anyhow::{anyhow, Context, Error, Result};
use xmp_toolkit::{xmp_ns, OpenFileOptions, XmpFile, XmpMeta};

const IMAGE_EXTENSIONS: [&str; 6] = ["heic", "jpg", "jpeg", "png", "arw", "dng"];
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

    #[arg(short = 'c', long, default_value_t = ComparisonCommand::MoreEqual)]
    comparison_command: ComparisonCommand,
}

#[derive(Subcommand, PartialEq)]
enum FileCommand {
    Move,
    Copy,
    Delete,
    Print,
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

    assert!(search_path.is_dir(), "Source path must be a directory");

    let output_path: Option<PathBuf> = cli.dest;

    if cli.command == FileCommand::Move || cli.command == FileCommand::Copy {
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

    let mut all_paths: Vec<PathBuf> = Vec::new();
    visit_dirs(
        search_path.as_ref(),
        &mut all_paths,
        0,
        cli.exclude,
        cli.flip_exclusion,
        cli.include_videos,
        cli.verbose,
    )
    .expect("Failed to iterate over directories");

    for path in all_paths {
        let relative_path = path
            .strip_prefix(search_path.clone())
            .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());

        let res: Result<i32> = get_rating(path.clone());
        let Ok(rating) = res else {
            println!(
                "Skipping {path:?} due to {}",
                res.err().unwrap_or(anyhow!("Unknown error")).to_string()
            );
            continue;
        };

        let pass_label_check = if let Some(ref label) = cli.label {
            let res: Result<Option<String>, String> = get_label(path.clone());
            let Ok(label_res) = res else {
                println!(
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
            let path_str = path.as_os_str().to_str().unwrap();

            if cli.verbose {
                println!("Rated: {rating} {command_name} {path:?}");
            }

            let mut new_file_path: Option<PathBuf> = None;
            if cli.command == FileCommand::Move || cli.command == FileCommand::Copy {
                new_file_path = Some(output_path.clone().unwrap().join(&relative_path));
                let new_file_path_clone = new_file_path.clone().unwrap();
                let dir_path: &Path = new_file_path_clone.parent().unwrap();
                if !path_exists(dir_path.to_path_buf()) {
                    fs::create_dir(dir_path.to_path_buf()).unwrap();
                }
            }

            apply_command(
                &cli.command,
                cli.verbose,
                path.clone(),
                new_file_path.clone(),
            );
            if cli.match_raws && (path_str.contains(".jpg") || path_str.contains(".JPG")) {
                let mut raw_path = path.clone();
                raw_path.set_extension("ARW");

                if raw_path.exists() {
                    if cli.verbose {
                        println!("Matched raw file {raw_path:?}");
                    }
                    let raw_relative_path = raw_path
                        .strip_prefix(search_path.clone())
                        .expect(format!("Failed to strip root prefix of file {:?}", path).as_str());
                    let new_raw_file_path: Option<PathBuf> = if output_path.is_none() {
                        None
                    } else {
                        Some(output_path.clone().unwrap().join(&raw_relative_path))
                    };
                    apply_command(&cli.command, cli.verbose, raw_path, new_raw_file_path);
                }
            }
        }
    }
}

fn apply_command(
    command: &FileCommand,
    verbose: bool,
    path: PathBuf,
    new_file_path: Option<PathBuf>,
) {
    match command {
        FileCommand::Move => {
            if new_file_path.clone().unwrap().exists() {
                if verbose {
                    println!("Skipping {new_file_path:?} as it already exists");
                }
            } else {
                if verbose {
                    let new_path_print = new_file_path.clone().unwrap();
                    println!("Moving {path:?} to {new_path_print:?}");
                }
                fs::rename(path, new_file_path.unwrap()).unwrap();
            }
        }
        FileCommand::Copy => {
            if new_file_path.clone().unwrap().exists() {
                if verbose {
                    println!("Skipping {new_file_path:?} as it already exists");
                }
            } else {
                if verbose {
                    let new_path_print = new_file_path.clone().unwrap();
                    println!("Copying {path:?} to {new_path_print:?}");
                }
                fs::copy(path, new_file_path.unwrap()).unwrap();
            }
        }
        FileCommand::Delete => {
            fs::remove_file(path).unwrap();
        }
        FileCommand::Print => {
            let path_str = path.as_os_str().to_str().unwrap();
            println!("{path_str}");
        }
    }
}

fn visit_dirs(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
    depth: i32,
    excluded_paths: Vec<String>,
    flip_exclusion: bool,
    include_videos: bool,
    print_directories: bool,
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
                    if print_directories && depth == 0 {
                        println!("Including {dir_name}");
                    }
                    visit_dirs(
                        &path,
                        paths,
                        depth + 1,
                        excluded_paths.clone(),
                        flip_exclusion,
                        include_videos,
                        print_directories,
                    )?;
                }
            } else {
                let path_buf = entry.path();
                if is_file_allowed(&path_buf, include_videos) {
                    println!("Adding {path_buf:?}");
                    paths.push(path_buf);
                } else {
                    println!("Skipping {path_buf:?}");
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
    
    if is_video(&filename){
       return read_rating_xmp(filename.clone());
    }

    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) => {
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
            Ok(rating)
        }
        Err(e) => anyhow::bail!(e),
    }
    
    // match read_xmp_from_file(filename) {
    //     Ok(rating) => {
    //         Ok(rating)
    //     }
    //     Err(e) => Err(e.to_string()),
    // }
}

fn is_video(path: &Path) -> bool {
    let extension = path.extension().unwrap_or_default().to_str().unwrap_or("").to_lowercase();
    VIDEOS_EXTENSIONS.contains(&extension.as_str())
}

struct CircularBuffer<T> {
    buffer: Vec<T>,
    index: usize,
}

impl<T: Default + Clone + Copy> CircularBuffer<T> {
    fn new(size: usize) -> CircularBuffer<T> {
        CircularBuffer {
            buffer: vec![Default::default(); size],
            index: 0,
        }
    }

    fn push(&mut self, value: T) {
        self.buffer[self.index] = value;
        self.index = (self.index + 1) % self.buffer.len();
    }

    fn get(&self, ith: usize) -> T {
        if ith >= self.buffer.len() {
            panic!("Index out of bounds");
        }
        self.buffer[(self.index + ith) % self.buffer.len()]
    }

    fn iter(&self) -> CircularBufferIterator<T> {
        CircularBufferIterator::<T> {
            buffer: self,
            current_index: 0,
        }
    }
}

struct CircularBufferIterator<'a, T> {
    buffer: &'a CircularBuffer<T>,
    current_index: usize,
}

impl<T: Copy + Default> Iterator for CircularBufferIterator<'_, T> {
    type Item = T;
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_index < self.buffer.buffer.len() {
            let value = self.buffer.get(self.current_index);
            self.current_index += 1;
            Some(value)
        } else {
            None
        }
    }
}

impl<T: Default + Clone + Copy + PartialEq> CircularBuffer<T> {
    fn contains(&self, subarray: &[T]) -> bool {
        if subarray.len() > self.buffer.len() {
            return false;
        }

        for (i, item) in self.iter().enumerate() {
            if item != subarray[i] {
                return false
            }
        }

        true
    }
}

const XMP_START: &[u8] = b"<x:xmpmeta";
const XMP_END: &[u8] = b"</x:xmpmeta>";
const XMP_SEARCH_BUFFER_SIZE: usize = 4096 * 16;
const XMP_END_SEARCH_SPACE_SIZE: usize = 4096 * 32;
const XMP_MAX_SEARCH_SPACE_SIZE: usize = 4096 * 1024;

fn read_rating_xmp(filename: PathBuf) -> Result<i32> {
    // println!("Printing XMP data for {:?}", filename);

    let xmp_data = extract_xmp_data(filename.clone(), true)?.or_else(|| extract_xmp_data(filename, false).unwrap());


    if xmp_data.is_none() {
        return Err(anyhow!("XMP data not found in the file."));
    }

    let xmp_meta = XmpMeta::from_str(std::str::from_utf8(&xmp_data.as_ref().unwrap()).unwrap());
    println!("{:?}", xmp_meta);
    let xmp_res: Result<i32> = Ok(xmp_meta.unwrap().property(xmp_ns::XMP, "Rating").map_or(0, |prop| {prop.value.parse::<i32>().unwrap()}));
    // Extract XMP data
    let own_res: Result<i32> = if let Some(value) = extract_rating_from_xmp_data(xmp_data.as_ref().unwrap())? {
        Ok(value)
    } else {
        Ok(0)
    };

    assert_eq!(xmp_res.is_err(), own_res.is_err());
    if let Ok(xmp_res) = xmp_res {
        if let Ok(own_res) = own_res {
            assert_eq!(xmp_res, own_res);
        }
    }

    xmp_res
}

fn extract_xmp_data(filename: PathBuf, read_from_end_of_file: bool) -> Result<Option<Vec<u8>>, Error> {
    let file = File::open(filename).unwrap();
    let mut reader = BufReader::new(file);
    let mut buffer = vec![0; XMP_SEARCH_BUFFER_SIZE];
    let mut total_bytes_read = 0;
    let mut start_buffer = CircularBuffer::new(XMP_START.len());
    let mut end_buffer = CircularBuffer::new(XMP_END.len());
    let mut start_found = false;
    let mut xmp_start_position = 0;
    let mut xmp_data = XMP_START.to_vec();
    if read_from_end_of_file {
        let file_size = reader.get_ref().metadata()?.len();
        // let offset = file_size.saturating_sub(XMP_END_SEARCH_SPACE_SIZE as u64);
        reader.seek(SeekFrom::End(-(XMP_END_SEARCH_SPACE_SIZE as i64)))?;
        // total_bytes_read = offset as usize;
    }

    while let Ok(n) = reader.read(&mut buffer) {
        if n == 0 {
            break;
        }
    
        for (i, &byte) in buffer.iter().take(n).enumerate() {
            if !start_found {
                start_buffer.push(byte);
            } else{
                end_buffer.push(byte);
            }
            total_bytes_read += 1;

            if total_bytes_read > XMP_MAX_SEARCH_SPACE_SIZE {
                return Ok(None);
            }

            if !start_found && start_buffer.contains(XMP_START) {
                start_found = true;
                xmp_start_position = total_bytes_read - XMP_START.len();
            }

            if start_found {
                xmp_data.push(byte);
            }

            if start_found && end_buffer.contains(XMP_END) {      
                return Ok(Some(xmp_data));
            }
        }
    }
    Ok(None)
}

fn extract_rating_from_xmp_data(xmp_data: &Vec<u8>) -> Result<Option<i32>> {
    let xmp_data = String::from_utf8_lossy(xmp_data);
    for line in xmp_data.lines() {
        if line.contains("xmp:Rating") {
            println!("XMP Rating: {:?}", line);
            let rating_str = if line.contains("=") {
                line.to_string().split('=').nth(1).unwrap().chars().filter(char::is_ascii_digit).collect::<String>()
            } else {
                line.to_string().split('>').nth(1).unwrap().split('<').nth(0).unwrap().chars().filter(char::is_ascii_digit).collect::<String>()
            };
            // println!("Rating str: {:?}", rating_str);
            return Ok(Some(rating_str.parse::<i32>().unwrap()));
        }
    }
    // println!("no rating found in XMP data: {:?}\n\n", xmp_data);
    return Ok(None);
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


// fn read_xmp_from_file(filename: PathBuf) -> Result<i32>  {
//     let mut f = XmpFile::new()?;

//     let path = filename.to_str().unwrap();

//     f.open_file(
//         path,
//         OpenFileOptions::default()
//     )
//     .or_else(|_err| {
//         // There might not be an appropriate handler available.
//         // Retry using packet scanning, providing a different set of
//         // open-file options.
//         eprintln!(
//             "No smart handler available for file {}. Trying packet scanning.",
//             path
//         );
//         f.open_file(path, OpenFileOptions::default().use_packet_scanning())
//     })
//     .with_context(|| format!("could not find XMP in file {}", path))?;

//     let xmp = f
//         .xmp()
//         .ok_or_else(|| anyhow!("unable to process XMP in file {}", path))?;

//     Ok(xmp.property(xmp_ns::XMP, "Rating").map_or(0, |prop| {prop.value.parse::<i32>().unwrap()}))
// }


fn is_file_allowed(filename: &PathBuf, include_videos: bool) -> bool {
    if filename.file_name().unwrap().to_string_lossy().starts_with(".") {
        return false;
    }

    let ext = filename
        .extension()
        .unwrap_or(OsStr::new(""))
        .to_str()
        .unwrap();
    let lower_passed = ext.to_lowercase();
    
    let ext: Vec<_> = match include_videos {
        // true => IMAGE_EXTENSIONS.iter().chain(VIDEOS_EXTENSIONS.iter()).collect::<Vec<_>>(),
        true => VIDEOS_EXTENSIONS.iter().collect(),
        false => IMAGE_EXTENSIONS.iter().collect(),
    };

    for allowed_extension in ext {
        let lower_allowed = allowed_extension.to_lowercase();
        if lower_allowed == lower_passed {
            return true;
        }
    }
    false
}
