use std::fs::{self, File};
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, Ok, Result};
use byteorder::{LE, ReadBytesExt};
use clap::Parser;
use image::{ImageFormat, RgbaImage, imageops};
use serde::Serialize;
use walkdir::{DirEntry, WalkDir};

#[derive(Parser)]
struct Cli {
    header_dir: PathBuf,
    raw_dir: PathBuf,
    out_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let header_dir = &cli.header_dir;

    for entry in WalkDir::new(header_dir) {
        if let Err(err) = handle_entry(&cli, entry) {
            eprintln!("Error: {err:?}");
        }
    }

    Ok(())
}

fn handle_entry(cli: &Cli, entry: walkdir::Result<DirEntry>) -> Result<()> {
    let header_dir = &cli.header_dir;
    let raw_dir = &cli.raw_dir;
    let out_dir = &cli.out_dir;
    let entry = entry?;

    if entry.file_type().is_dir() {
        return Ok(());
    }

    let path = entry.path();
    let extension = path.extension().and_then(|ext| ext.to_str());

    if extension != Some("header") {
        return Ok(());
    }

    let relative_path = path
        .strip_prefix(header_dir)
        .with_context(|| format!("{path:?}"))?;

    let mut raw_path = raw_dir.join(relative_path);
    raw_path.set_extension("raw");

    let mut out_dir = out_dir.join(relative_path);
    out_dir.set_extension("");

    fs::create_dir_all(&out_dir).with_context(|| format!("{out_dir:?}"))?;

    let bank_header = BankHeader::from_path(path).with_context(|| format!("{path:?}"))?;

    let raw_data = File::open(&raw_path).with_context(|| format!("{raw_path:?}"))?;
    let mut raw_data = BufReader::new(raw_data);

    for (index, entry) in bank_header.entries().enumerate() {
        if let Err(err) = save_header(&out_dir, index, entry) {
            eprintln!("failed to save header for texture {index} of {raw_path:?}: {err:?}");
            continue;
        }

        if let Err(err) = save_texture(&out_dir, &mut raw_data, index, entry) {
            eprintln!("failed to save texture {index} of {raw_path:?}: {err:?}");
            continue;
        }
    }

    Ok(())
}

fn save_header(out_dir: &Path, index: usize, entry: &TextureInfo) -> Result<()> {
    let out_path = out_dir.join(format!("{index:02}.json"));
    let header_json_path = out_path.with_extension("json");
    let header_json = serde_json::to_string_pretty(entry)?;

    fs::write(header_json_path, header_json)?;

    Ok(())
}

fn save_texture(
    out_dir: &Path,
    raw_data: &mut BufReader<File>,
    index: usize,
    entry: &TextureInfo,
) -> Result<(), anyhow::Error> {
    let mut image = entry.load_texture_from_reader(raw_data)?;
    let out_path = out_dir.join(format!("{index:02}.png"));

    imageops::flip_vertical_in_place(&mut image);

    image
        .save_with_format(&out_path, ImageFormat::Png)
        .with_context(|| format!("{out_path:?}"))?;

    Ok(())
}

pub struct BankHeader {
    entries: Vec<TextureInfo>,
}

impl BankHeader {
    fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let header = fs::read(path)?;
        let mut header = Cursor::new(header);
        let entries = TextureInfo::all_from_reader(&mut header)?;

        Ok(Self { entries })
    }

    fn entries(&self) -> impl Iterator<Item = &TextureInfo> {
        self.entries.iter()
    }
}

#[derive(Serialize)]
pub struct TextureInfo {
    width: u16,
    height: u16,
    pixel_format: u8,
    _unk0: u8,
    _unk1: u16,
    offset: u32,
    texture_id: u16,
    _unk4: u16,
}

impl TextureInfo {
    fn all_from_reader<R>(reader: &mut R) -> Result<Vec<TextureInfo>>
    where
        R: BufRead + Seek,
    {
        let size = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;

        let num_headers = (size / 16) as usize;
        let mut headers = Vec::with_capacity(num_headers);

        for _ in 0..num_headers {
            let header = Self::from_reader(reader)?;

            headers.push(header);
        }

        Ok(headers)
    }

    fn from_reader<R: Read>(r: &mut R) -> Result<Self> {
        Ok(Self {
            width: r.read_u16::<LE>()?,
            height: r.read_u16::<LE>()?,
            pixel_format: r.read_u8()?,
            _unk0: r.read_u8()?,
            _unk1: r.read_u16::<LE>()?,
            offset: r.read_u32::<LE>()?,
            texture_id: r.read_u16::<LE>()?,
            _unk4: r.read_u16::<LE>()?,
        })
    }

    pub fn load_texture_from_reader<R>(&self, reader: &mut R) -> Result<RgbaImage>
    where
        R: BufRead + Seek,
    {
        reader.seek(SeekFrom::Start(self.offset as u64))?;

        let pixels = self.read_pixel_data(reader)?;
        let image = RgbaImage::from_vec(self.width as u32, self.height as u32, pixels)
            .context("buffer too small")?;

        Ok(image)
    }

    fn read_pixel_data<R: Read>(&self, reader: &mut R) -> Result<Vec<u8>> {
        let num_pixels = self.width as usize * self.height as usize;
        let num_bytes = num_pixels * 4; // RGBA8888
        let mut pixels = Vec::with_capacity(num_bytes);

        for _ in 0..num_pixels {
            // TODO: respect pixel format
            let (g, b) = read_44_pixel(reader)?;
            let (a, r) = read_44_pixel(reader)?;

            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
            pixels.push(a);
        }

        Ok(pixels)
    }
}

fn read_44_pixel<R: Read>(r: &mut R) -> Result<(u8, u8)> {
    let byte = r.read_u8()?;
    let high = scale_4bit_to_8bit((byte >> 4) & 0b1111);
    let low = scale_4bit_to_8bit((byte >> 0) & 0b1111);

    Ok((high, low))
}

fn scale_4bit_to_8bit(nibble: u8) -> u8 {
    ((nibble as f32 / 15.) * 255.) as u8
}
