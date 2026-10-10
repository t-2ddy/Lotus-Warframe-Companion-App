//! Text recognition with Windows' built-in OCR (`Windows.Media.Ocr`): no download, no extra
//! library. Reads a riven card in ~20 ms. Uses the user's Windows display language (the overlay
//! expects an English game and English OCR for now).

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrResult};
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::screen::Image;

thread_local! {
    /// One engine per thread (the overlay reads on its own thread), created on first use.
    static ENGINE: Option<OcrEngine> = {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        OcrEngine::TryCreateFromUserProfileLanguages().ok()
    };
}

/// One recognised word and where it is in the image (pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    /// Which OCR line it belongs to (lines run top to bottom).
    pub line: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Reads the words in an image with their positions (for screens with several text columns).
pub fn read_words(img: &Image) -> Result<Vec<Word>, String> {
    let e = |err: windows::core::Error| err.message().to_string();
    let lines = recognise(img)?.Lines().map_err(e)?;
    let mut out = Vec::new();
    for i in 0..lines.Size().map_err(e)? {
        let words = lines.GetAt(i).map_err(e)?.Words().map_err(e)?;
        for j in 0..words.Size().map_err(e)? {
            let word = words.GetAt(j).map_err(e)?;
            let r = word.BoundingRect().map_err(e)?;
            out.push(Word {
                text: word.Text().map_err(e)?.to_string(),
                line: i as usize,
                x: f64::from(r.X),
                y: f64::from(r.Y),
                w: f64::from(r.Width),
                h: f64::from(r.Height),
            });
        }
    }
    Ok(out)
}

/// Reads the text lines in an image.
pub fn read_lines(img: &Image) -> Result<Vec<String>, String> {
    let e = |err: windows::core::Error| err.message().to_string();
    let lines = recognise(img)?.Lines().map_err(e)?;
    let mut out = Vec::new();
    for i in 0..lines.Size().map_err(e)? {
        out.push(lines.GetAt(i).map_err(e)?.Text().map_err(e)?.to_string());
    }
    Ok(out)
}

fn recognise(img: &Image) -> Result<OcrResult, String> {
    ENGINE.with(|engine| {
        let engine = engine.as_ref().ok_or("Windows OCR isn't available for your display language")?;
        let e = |err: windows::core::Error| err.message().to_string();
        let writer = DataWriter::new().map_err(e)?;
        writer.WriteBytes(&img.bgra).map_err(e)?;
        let buffer = writer.DetachBuffer().map_err(e)?;
        let bitmap =
            SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, img.width as i32, img.height as i32)
                .map_err(e)?;
        engine.RecognizeAsync(&bitmap).map_err(e)?.join().map_err(e)
    })
}

/// Decodes an image file to BGRA (tests read saved screenshots with it).
#[cfg(test)]
pub fn decode_file(path: &std::path::Path) -> Result<Image, String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::Streams::InMemoryRandomAccessStream;
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    let e = |err: windows::core::Error| err.message().to_string();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let stream = InMemoryRandomAccessStream::new().map_err(e)?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(e)?;
    writer.WriteBytes(&bytes).map_err(e)?;
    writer.StoreAsync().map_err(e)?.join().map_err(e)?;
    writer.DetachStream().map_err(e)?;
    stream.Seek(0).map_err(e)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).map_err(e)?.join().map_err(e)?;
    let (w, h) = (decoder.PixelWidth().map_err(e)?, decoder.PixelHeight().map_err(e)?);
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Ignore,
            &BitmapTransform::new().map_err(e)?,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .map_err(e)?
        .join()
        .map_err(e)?
        .DetachPixelData()
        .map_err(e)?;
    Ok(Image { width: w, height: h, bgra: pixels.to_vec() })
}
