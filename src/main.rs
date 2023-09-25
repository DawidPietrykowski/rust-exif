use std::{fs, io};
use std::ffi::OsStr;
use std::fs::DirEntry;
use std::path::{Path, PathBuf};
use exif::{Context, In, Tag};
use rexiv2::Metadata;

const ALLOWED_EXTENSIONS: [&str; 6] = ["heic", "jpg", "jpeg", "png", "arw", "dng"];

fn is_image(filename: &PathBuf) -> bool{
    if filename.file_name().unwrap().to_str().unwrap().starts_with("."){
        return false;
    }
    let ext = filename.extension().unwrap_or(OsStr::new("")).to_str().unwrap();
    let lower_passed = ext.to_lowercase();
    for allowed_extension in ALLOWED_EXTENSIONS {
        let lower_allowed = allowed_extension.to_lowercase();
        if lower_allowed == lower_passed{
            return true;
        }
    }
    false
}

fn visit_dirs(dir: &Path, paths: &mut Vec<PathBuf>, depth: i32, x: fn(&str) -> bool) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.as_path().file_name().expect("Could not get relative path").to_str().unwrap();
                if (depth != 0 || x(dir_name)) && !dir_name.starts_with("."){
                    // filter
                    if depth == 0 {
                        println!("Passed {dir_name}");
                    }
                    visit_dirs(&path, paths, depth + 1, x)?;
                }
            } else {
                let path_buf = entry.path();
                if is_image(&path_buf)
                {
                    paths.push(path_buf);
                }
            }
        }
    }
    Ok(())
}

fn starts_with_2210(string: &str) -> bool {
    // (string.starts_with("23") || string.starts_with("22")
    //     || string.starts_with("21-11") || string.starts_with("21-09"))
    //     && !string.contains("21-09-00-Warsaw")
    string.contains("KomuniaKuby")
}

fn main() {
    rexiv2::initialize().expect("Unable to initialize rexiv2");
    let mut all_paths: Vec<PathBuf> = Vec::new();
    let search_path = "/media/nfs/sphotos/Images/22-07-01-London/";
    let output_path = "/media/nfs/tmp/22-07-01-London-5star/";
    visit_dirs(search_path.as_ref(), &mut all_paths, 1, |_| true).expect("TODO: panic message");
    for path in all_paths{
        let filename = path.as_path().to_str().unwrap();
        let mut root_directory = path.as_os_str().to_str().unwrap().chars().skip(search_path.len()).collect::<String>();
        if let Some(pos) = root_directory.find("/"){
            root_directory = root_directory.chars().take(pos).collect::<String>();
        }
        let res = get_rating(filename.clone());
        let Ok(rating) = res else{
            println!("Skipping {filename} due to {}", res.err().unwrap_or("Unknown error".to_string()).to_string());
            continue;
        };
        if rating >= 5{
            let out_path = output_path.to_string() + &root_directory + "/" + path.clone().file_name().unwrap().to_str().to_owned().unwrap();
            println!("{rating} {root_directory} {filename} {out_path}");
            let dir_path = (output_path.to_string() + &root_directory);
            if !path_exists(dir_path.as_str()){
                fs::create_dir(dir_path.as_str()).unwrap();
            }
            if !path_exists(out_path.as_str()){
                fs::copy(path.clone(), Path::new(out_path.as_str())).unwrap();
            }
            if !path.as_os_str().to_str().unwrap().contains("HEIC"){
                let jpg_suffix = match path.as_os_str().to_str().unwrap().contains(".JPG") {
                    true => ".JPG",
                    false => "-c.jpg"
                };
                let raw_path = (path.as_os_str().to_str().unwrap().strip_suffix(jpg_suffix).unwrap().to_string() + ".ARW");
                let raw_path_str = raw_path.as_str();
                if path_exists(raw_path_str) {
                    let out_path_raw = out_path.strip_suffix(jpg_suffix).unwrap().to_string() + ".ARW";
                    fs::copy(raw_path_str, Path::new(out_path_raw.as_str())).unwrap();
                }
            }
        }
    }
}

pub fn path_exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}

fn get_rating(filename: &str) -> Result<i32, String> {
    if !path_exists(filename){
        return Err("File doesn't exist".to_string());
    }

    let meta = Metadata::new_from_path(filename);
    match meta {
        Ok(meta) =>{
            let rating = meta.get_tag_numeric("Xmp.xmp.Rating");
                // println!("{rating:?} {filename}");
            Ok(rating)
        },
        Err(e) =>{
            Err(e.to_string())
        }
    }

}
