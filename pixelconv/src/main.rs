use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use bitstream_reader::{BitBuffer, BitStream, LittleEndian};
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// raw BGRA4444 encoded input
    path: PathBuf,
    /// raw RGBA8888 enecoded output
    out: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = fs::read(cli.path)?;
    let mut reader = BitStream::new(BitBuffer::new(data, LittleEndian));
    let mut out = Vec::new();

    while reader.bits_left() > 0 {
        let b = read_pixel(&mut reader);
        let g = read_pixel(&mut reader);
        let r = read_pixel(&mut reader);
        let a = read_pixel(&mut reader);

        out.push(r);
        out.push(g);
        out.push(b);
        out.push(a);
    }

    fs::write("out.data", out.as_slice())?;

    Ok(())
}

fn read_pixel(reader: &mut BitStream<LittleEndian>) -> u8 {
    let pixel = reader.read_int::<u8>(4).unwrap();
    let pixel = (pixel as f32 / 15.) * 255.;

    pixel as u8
}
