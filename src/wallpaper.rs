use color_eyre::Result;
use image::DynamicImage;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

pub struct Wallpaper {
    pub path: PathBuf,
    pub name: String,
    pub thumbnail: Option<DynamicImage>,
}

impl Wallpaper {
    pub fn new(path: PathBuf) -> Self {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        Self { path, name, thumbnail: None }
    }

    pub fn load_thumbnail(&mut self) {
        if self.thumbnail.is_some() {
            return;
        }

        // Try freedesktop thumbnails first (x-large, large, normal)
        if let Some(thumb) = load_freedesktop_thumbnail(&self.path) {
            self.thumbnail = Some(thumb);
            return;
        }

        // Fallback: load original and resize
        if let Ok(img) = image::open(&self.path) {
            let thumb = img.thumbnail(512, 512);
            self.thumbnail = Some(thumb);
        }
    }
}

fn get_freedesktop_thumb_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".cache"))
        .join("thumbnails")
}

fn get_freedesktop_thumbnail_path(original: &PathBuf, size: &str) -> PathBuf {
    // Freedesktop spec: MD5 hash of file URI
    let uri = format!("file://{}", original.canonicalize().unwrap_or(original.clone()).display());
    let hash = format!("{:x}", md5::compute(uri.as_bytes()));
    get_freedesktop_thumb_dir().join(size).join(format!("{}.png", hash))
}

fn load_freedesktop_thumbnail(original: &PathBuf) -> Option<DynamicImage> {
    // Try sizes from largest to smallest
    for size in &["xx-large", "x-large", "large", "normal"] {
        let thumb_path = get_freedesktop_thumbnail_path(original, size);
        if thumb_path.exists() {
            if let Ok(img) = image::open(&thumb_path) {
                return Some(img);
            }
        }
    }
    None
}

pub fn get_backgrounds_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/omarchy/current/theme/backgrounds")
}

pub fn get_current_background_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/omarchy/current/background")
}

pub fn discover_wallpapers(dir: Option<PathBuf>) -> Result<Vec<Wallpaper>> {
    let backgrounds_dir = dir.unwrap_or_else(get_backgrounds_dir);
    let mut wallpapers = Vec::new();

    if backgrounds_dir.exists() {
        for entry in fs::read_dir(&backgrounds_dir)? {
            let entry = entry?;
            let path = entry.path();
            if is_image(&path) {
                wallpapers.push(Wallpaper::new(path));
            }
        }
    }

    wallpapers.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(wallpapers)
}

pub fn get_current_wallpaper() -> Option<PathBuf> {
    let current = get_current_background_path();
    fs::read_link(&current).ok()
}

pub fn install_wallpaper(wallpaper: &Wallpaper) -> Result<PathBuf> {
    let backgrounds_dir = get_backgrounds_dir();
    if !backgrounds_dir.exists() {
        fs::create_dir_all(&backgrounds_dir)?;
    }

    let file_name = wallpaper
        .path
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("Invalid file name"))?;
    let dest_path = backgrounds_dir.join(file_name);

    if wallpaper.path != dest_path {
        fs::copy(&wallpaper.path, &dest_path)?;
    }

    Ok(dest_path)
}

pub fn set_wallpaper(path: &PathBuf) -> Result<()> {
    let current = get_current_background_path();

    // Remove existing symlink
    if current.exists() || current.is_symlink() {
        fs::remove_file(&current)?;
    }

    // Create new symlink
    symlink(path, &current)?;

    // Reload swaybg
    reload_swaybg()?;

    Ok(())
}

fn reload_swaybg() -> Result<()> {
    // Kill existing swaybg
    let _ = Command::new("killall").arg("swaybg").output();

    // Start new swaybg
    Command::new("swaybg")
        .arg("-i")
        .arg(get_current_background_path())
        .arg("-m")
        .arg("fill")
        .spawn()?;

    Ok(())
}

fn is_image(path: &PathBuf) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp"
        ),
        None => false,
    }
}
