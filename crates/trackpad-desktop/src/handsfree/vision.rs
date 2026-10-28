//! Screenshot capture and the SAM 3.1 seam.
//!
//! `segment_screen` calls [`Segmenter`]. [`Sam31Stub`] is the stand-in until
//! a later change posts the captured image to `sam-3.1` on the Responses API.
//! The trait is the only thing that change needs to replace.

/// One desktop frame. `rgba` length is `width * height * 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CapturedImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| "screenshot is too large".to_string())?;
        if rgba.len() != expected {
            return Err(format!(
                "screenshot byte length {} does not match {width}x{height}",
                rgba.len()
            ));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }
}

/// A box Spark can aim at once SAM 3.1 is connected. Coordinates are pixels
/// in the captured image, origin top-left. They are not sent on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiSegment {
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub trait ScreenCapture {
    fn capture(&mut self) -> Result<CapturedImage, String>;
}

pub trait Segmenter {
    fn segment(&mut self, image: &CapturedImage, prompt: &str) -> Result<Vec<UiSegment>, String>;
}

/// Used by tests and by the offline demo when a real grab is unnecessary.
#[derive(Debug, Clone)]
pub struct MemoryCapture {
    pub image: CapturedImage,
    pub calls: u32,
}

impl ScreenCapture for MemoryCapture {
    fn capture(&mut self) -> Result<CapturedImage, String> {
        self.calls += 1;
        Ok(self.image.clone())
    }
}

/// Desktop grab. Windows uses GDI. Other builds return a clear error so the
/// tool loop can continue with relative motion.
pub struct PlatformCapture;

impl ScreenCapture for PlatformCapture {
    fn capture(&mut self) -> Result<CapturedImage, String> {
        #[cfg(windows)]
        {
            capture_windows()
        }
        #[cfg(not(windows))]
        {
            Err(
                "screenshot capture is implemented for Windows in this build. \
                 The ScreenCapture trait is the seam."
                    .into(),
            )
        }
    }
}

/// SAM 3.1 is intentionally not called here.
///
/// Next step: `POST https://api.meta.ai/v1/responses` with `model` `sam-3.1`,
/// one user message, an `input_text` noun phrase, and an `input_image` data
/// URL of this screenshot. Read the boxes and return [`UiSegment`]s.
pub struct Sam31Stub;

impl Segmenter for Sam31Stub {
    fn segment(&mut self, image: &CapturedImage, prompt: &str) -> Result<Vec<UiSegment>, String> {
        let _ = image;
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(
                "segment_screen needs a short noun phrase, such as \"submit button\"".into(),
            );
        }
        Err(format!(
            "SAM 3.1 is not connected yet (model sam-3.1). Captured {}x{} for {prompt:?}. \
             Keep using move_pointer and click.",
            image.width, image.height
        ))
    }
}

#[cfg(windows)]
fn capture_windows() -> Result<CapturedImage, String> {
    use std::os::raw::c_void;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 || width > 16_000 || height > 16_000 {
            return Err(format!("screen size {width}x{height} is not usable"));
        }
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("GetDC failed".into());
        }
        let mem = CreateCompatibleDC(Some(screen));
        if mem.is_invalid() {
            let _ = ReleaseDC(None, screen);
            return Err("CreateCompatibleDC failed".into());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap.is_invalid() {
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return Err("CreateCompatibleBitmap failed".into());
        }
        let previous = SelectObject(mem, bitmap.into());
        let blitted = BitBlt(mem, 0, 0, width, height, Some(screen), 0, 0, SRCCOPY);
        if blitted.is_err() {
            SelectObject(mem, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return Err("BitBlt failed".into());
        }
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default()],
        };
        let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
        let rows = GetDIBits(
            mem,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast::<c_void>()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);
        if rows == 0 {
            return Err("GetDIBits failed".into());
        }
        let mut rgba = Vec::with_capacity(bgra.len());
        for px in bgra.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        CapturedImage::new(width as u32, height as u32, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_names_sam_and_keeps_the_image_size() {
        let image = CapturedImage::new(2, 1, vec![0, 0, 0, 255, 1, 2, 3, 255]).unwrap();
        let err = Sam31Stub.segment(&image, "submit button").unwrap_err();
        assert!(err.contains("sam-3.1"), "{err}");
        assert!(err.contains("2x1"), "{err}");
        assert!(Sam31Stub.segment(&image, "  ").is_err());
    }

    #[test]
    fn memory_capture_returns_the_same_frame() {
        let image = CapturedImage::new(1, 1, vec![9, 8, 7, 255]).unwrap();
        let mut cap = MemoryCapture {
            image: image.clone(),
            calls: 0,
        };
        assert_eq!(cap.capture().unwrap(), image);
        assert_eq!(cap.calls, 1);
    }
}
