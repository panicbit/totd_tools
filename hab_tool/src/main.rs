use core::slice;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::iter;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use clap::Parser;

#[derive(Parser)]
pub struct Cli {
    file: PathBuf,
    out_dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let file = File::open(cli.file)?;
    let file = BufReader::new(file);
    let mut hab = Hab::new(file)?;

    let out_dir = cli.out_dir;
    fs::create_dir_all(&out_dir)?;

    for i in 0..hab.num_entries() {
        let mut hab_file = hab.get_file_by_index(i)?;
        let mut out_file = File::create(out_dir.join(hab_file.file_name()))?;

        io::copy(&mut hab_file, &mut out_file)?;
    }

    // eprintln!("{hab:#?}");

    Ok(())
}

#[derive(Debug)]
struct Hab<R> {
    reader: R,
    entries: Vec<FileEntry>,
    total_size: u32,
    data_start: u64,
}

impl<R> Hab<R>
where
    R: BufRead + Seek,
{
    pub fn new(mut reader: R) -> Result<Self> {
        reader.read_magic()?;
        let _unknown = reader.read_bytes(16)?;
        let num_entries = reader.read_u16()? as usize;
        let _unknown = reader.read_u16()?;
        let _unknown = reader.read_u32()?;
        let total_size = reader.read_u32()?;
        let mut file_metas = Vec::new();

        for _ in 0..num_entries {
            let entry = FileMeta::from_reader(&mut reader)?;

            file_metas.push(entry);
        }

        // eprintln!("{:#?}", file_metas);

        let filenames_start = reader.stream_position()?;
        let mut entries = Vec::new();

        for meta in file_metas {
            let mut name = Vec::new();
            reader.seek(SeekFrom::Start(filenames_start + meta.name_offset))?;
            reader.read_until(0, &mut name)?;
            name.pop();

            let name = String::from_utf8(name)?;

            println!("filename: {name}");

            entries.push(FileEntry { name, meta });
        }

        let data_start = reader.stream_position()?;

        Ok(Self {
            reader,
            total_size,
            entries,
            data_start,
        })
    }

    fn num_entries(&self) -> usize {
        self.entries.len()
    }

    fn get_file_by_index(&mut self, index: usize) -> Result<HabFile<R>> {
        let entry = self.entries.get(index).context("invalid entry index")?;

        HabFile::new(&mut self.reader, entry, self.data_start)
    }
}

struct HabFile<'a, R> {
    reader: io::Take<&'a mut R>,
    entry: &'a FileEntry,
}

impl<'a, R> HabFile<'a, R>
where
    R: BufRead + Seek,
{
    fn file_name(&self) -> &str {
        &self.entry.name
    }

    fn new(reader: &'a mut R, entry: &'a FileEntry, data_start: u64) -> Result<Self> {
        reader.seek(SeekFrom::Start(data_start + entry.meta.data_offset))?;

        Ok(Self {
            reader: reader.take(entry.meta.data_size),
            entry,
        })
    }
}

impl<R> Read for HabFile<'_, R>
where
    R: BufRead,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<R> BufRead for HabFile<'_, R>
where
    R: BufRead,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.reader.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.reader.consume(amt);
    }
}

#[derive(Debug)]
struct FileMeta {
    name_offset: u64,
    data_offset: u64,
    data_size: u64,
    // unknown: u32,
}

impl FileMeta {
    fn from_reader<R>(r: &mut R) -> Result<Self>
    where
        R: BufRead + Seek,
    {
        let name_offset = r.read_u32()? as u64;
        let data_offset = r.read_u32()? as u64;
        let data_size = r.read_u32()? as u64;
        let _unknown = r.read_u32()?;

        Ok(Self {
            name_offset,
            data_offset,
            data_size,
        })
    }
}

#[derive(Debug)]
struct FileEntry {
    name: String,
    meta: FileMeta,
}

trait HabReader: Read + Seek {
    fn read_magic(&mut self) -> Result<()> {
        const MAGIC: &[u8] = b"HAB0";

        let mut buf = [0; MAGIC.len()];
        self.read_exact(&mut buf)?;

        ensure!(buf == MAGIC);

        Ok(())
    }

    fn read_bytes(&mut self, amount: usize) -> Result<Vec<u8>> {
        let mut bytes = vec![0; amount];

        self.read_exact(&mut bytes)?;

        Ok(bytes)
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0; 4];

        self.read_exact(&mut buf)?;

        Ok(u32::from_le_bytes(buf))
    }

    fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0; 2];

        self.read_exact(&mut buf)?;

        Ok(u16::from_le_bytes(buf))
    }

    fn read_file_entry(&mut self) -> Result<FileMeta> {
        let name_offset = self.read_u32()? as u64;
        let data_offset = self.read_u32()? as u64;
        let data_size = self.read_u32()? as u64;
        let _unknown = self.read_u32()?;

        Ok(FileMeta {
            name_offset,
            data_offset,
            data_size,
        })
    }
}

impl<R: Read + Seek> HabReader for R {}
