//! Image loaders put over egui_extras' ones: an image never makes Florm read another file than
//! the one it shows (an SVG `<image href="/dev/zero">`, a link to a device…).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use eframe::egui;
use egui::load::{BytesLoadResult, BytesLoader, BytesPoll, ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};

/// Installs the loaders; to call after `egui_extras::install_image_loaders` (the last ones added
/// are asked first).
pub fn install(ctx: &egui::Context) {
    ctx.add_bytes_loader(Arc::new(FileGuard::default()));
    ctx.add_image_loader(Arc::new(SafeSvgLoader::default()));
}

/// The path of a `file://` address, as egui_extras' file loader reads it.
fn file_path(uri: &str) -> Option<&str> {
    let p = uri.strip_prefix("file://")?;
    Some(if cfg!(windows) { p.trim_start_matches('/') } else { p })
}

/// Refuses the `file://` addresses that are not a regular file (a device such as /dev/zero, a
/// pipe: read without end); the others go on to egui_extras' file loader.
#[derive(Default)]
struct FileGuard {
    checked: Mutex<HashMap<String, bool>>,
}

impl BytesLoader for FileGuard {
    fn id(&self) -> &str {
        egui::generate_loader_id!(FileGuard)
    }

    fn load(&self, _ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        let Some(path) = file_path(uri) else { return Err(LoadError::NotSupported) };
        let mut checked = self.checked.lock().unwrap();
        // metadata follows links: a link to a device is refused too
        let ok = *checked.entry(uri.to_string()).or_insert_with(|| std::fs::metadata(path).map_or(true, |m| m.is_file()));
        if ok {
            Err(LoadError::NotSupported)
        } else {
            Err(LoadError::Loading(t!("pas un fichier ordinaire", "not a regular file").into()))
        }
    }

    fn forget(&self, uri: &str) {
        self.checked.lock().unwrap().remove(uri);
    }

    fn forget_all(&self) {
        self.checked.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        0
    }
}

/// SVG options whose `<image>` only takes images written inside the file (`data:` PNG, JPEG,
/// GIF): never a path, which usvg would read from the disk by default.
fn svg_options() -> resvg::usvg::Options<'static> {
    use resvg::usvg::{ImageHrefResolver, ImageKind};
    let mut opt = resvg::usvg::Options::default();
    opt.image_href_resolver = ImageHrefResolver {
        resolve_data: Box::new(|mime, data, _| {
            let d = data.as_slice();
            match mime {
                "image/png" => Some(ImageKind::PNG(data)),
                "image/jpg" | "image/jpeg" => Some(ImageKind::JPEG(data)),
                "image/gif" => Some(ImageKind::GIF(data)),
                // a nested SVG would be read with usvg's default options (paths allowed): left out
                "text/plain" if d.starts_with(b"\x89PNG") => Some(ImageKind::PNG(data)),
                "text/plain" if d.starts_with(b"\xFF\xD8\xFF") => Some(ImageKind::JPEG(data)),
                "text/plain" if d.starts_with(b"GIF8") => Some(ImageKind::GIF(data)),
                _ => None,
            }
        }),
        resolve_string: Box::new(|_, _| None),
    };
    opt
}

/// An SVG drawn at the size asked (by egui_extras, with `svg_options`).
pub fn rasterize_svg(bytes: &[u8], size_hint: SizeHint) -> Result<egui::ColorImage, String> {
    egui_extras::image::load_svg_bytes_with_size(bytes, size_hint, &svg_options())
}

type Entry = Result<Arc<egui::ColorImage>, String>;

/// egui_extras' SVG loader, with `svg_options`.
#[derive(Default)]
struct SafeSvgLoader {
    cache: Mutex<HashMap<(String, SizeHint), Entry>>,
}

impl ImageLoader for SafeSvgLoader {
    fn id(&self) -> &str {
        egui::generate_loader_id!(SafeSvgLoader)
    }

    fn load(&self, ctx: &egui::Context, uri: &str, size_hint: SizeHint) -> ImageLoadResult {
        if Path::new(uri).extension().and_then(|e| e.to_str()) != Some("svg") {
            return Err(LoadError::NotSupported);
        }
        let key = (uri.to_string(), size_hint);
        if let Some(entry) = self.cache.lock().unwrap().get(&key).cloned() {
            return entry.map(|image| ImagePoll::Ready { image }).map_err(LoadError::Loading);
        }
        match ctx.try_load_bytes(uri) {
            Ok(BytesPoll::Ready { bytes, .. }) => {
                let entry = rasterize_svg(&bytes, size_hint).map(Arc::new);
                self.cache.lock().unwrap().insert(key, entry.clone());
                entry.map(|image| ImagePoll::Ready { image }).map_err(LoadError::Loading)
            }
            Ok(BytesPoll::Pending { size }) => Ok(ImagePoll::Pending { size }),
            Err(e) => Err(e),
        }
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().unwrap().retain(|(u, _), _| u != uri);
    }

    fn forget_all(&self) {
        self.cache.lock().unwrap().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache.lock().unwrap().values().map(|e| e.as_ref().map_or(0, |i| i.pixels.len() * 4)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `<image>` pointing at a file of the disk is not read (a device would hang Florm); one
    /// written in the file (`data:`) is still given to the renderer.
    #[test]
    fn svg_images_never_read_the_disk() {
        let dir = std::env::temp_dir().join(format!("florm_svg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("red.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255])).save(&png).unwrap();
        let opt = svg_options();
        let r = &opt.image_href_resolver;
        for href in [png.display().to_string(), format!("file://{}", png.display()), "/dev/zero".into(), "red.png".into()] {
            assert!((r.resolve_string)(&href, &opt).is_none(), "{href}");
        }
        let bytes = std::fs::read(&png).unwrap();
        assert!((r.resolve_data)("image/png", Arc::new(bytes.clone()), &opt).is_some());
        assert!((r.resolve_data)("text/plain", Arc::new(bytes), &opt).is_some());
        // a nested SVG (usvg would read it with its default options, paths allowed): left out
        assert!((r.resolve_data)("image/svg+xml", Arc::new(b"<svg/>".to_vec()), &opt).is_none());
        // a whole SVG pointing at a device: drawn at once, without reading it
        #[cfg(unix)]
        {
            let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="4" height="4"><image width="4" height="4" xlink:href="/dev/zero"/></svg>"#;
            let img = rasterize_svg(svg.as_bytes(), SizeHint::Scale(1.0.into())).unwrap();
            assert_eq!(img.size, [4, 4]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn devices_are_not_read() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        install(&ctx);
        assert!(matches!(ctx.try_load_bytes("file:///dev/zero"), Err(LoadError::Loading(_))));
        let dir = std::env::temp_dir().join(format!("florm_dev_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("img.png");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        assert!(matches!(ctx.try_load_bytes(&format!("file://{}", link.display())), Err(LoadError::Loading(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
