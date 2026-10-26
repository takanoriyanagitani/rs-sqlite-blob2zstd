use std::io;
use std::path::Path;

use rusqlite::Row;
use rusqlite::Rows;

use rusqlite::types::ValueRef;

use rusqlite::Connection;
use rusqlite::OpenFlags;

use rusqlite::Statement;
use rusqlite::Transaction;

use zrip::CompressContext;
use zrip::Dictionary;

pub fn realloc(buf: &mut Vec<u8>, size: usize) {
    let cur: usize = buf.len();
    if size <= cur {
        return;
    }

    buf.resize(size, 0)
}

pub fn blob2zstd<'a>(
    c: &mut CompressContext,
    blob: &[u8],
    buf: &'a mut Vec<u8>,
) -> Result<&'a [u8], io::Error> {
    let max_buf_size: usize = zrip::compress_bound(blob.len());
    realloc(buf, max_buf_size);
    let out: &mut [u8] = buf;
    let out_size: usize = c.compress_into(blob, out).map_err(io::Error::other)?;
    let o: &[u8] = &out[..out_size];
    Ok(o)
}

pub fn blobs2zstd(
    converter: &mut CompressContext,
    blob_column_index: usize,
    i: &mut Statement,
    o: &mut Statement,
    buf: &mut Vec<u8>,
) -> Result<usize, io::Error> {
    let column_count: usize = i.column_count();
    let mut rows: Rows = i.query(()).map_err(io::Error::other)?;
    let mut tot: usize = 0;
    loop {
        let orow: Option<&Row> = rows.next().map_err(io::Error::other)?;
        let Some(row) = orow else {
            return Ok(tot);
        };

        let vblob: ValueRef = row.get_ref(blob_column_index).map_err(io::Error::other)?;
        let oblob: Option<&[u8]> = match vblob {
            ValueRef::Null => Ok(None),
            ValueRef::Blob(s) | ValueRef::Text(s) => Ok(Some(s)),
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "not a blob")),
        }?;
        buf.clear();
        let ozstd: Option<&[u8]> = match oblob {
            None => Ok(None),
            Some(blob) => blob2zstd(converter, blob, buf).map(Some),
        }?;

        for i in 0..column_count {
            let vref: ValueRef = row.get_ref(i).map_err(io::Error::other)?;
            if i == blob_column_index {
                let Some(zstd) = ozstd else { continue };

                o.raw_bind_parameter(i + 1, zstd)
                    .map_err(io::Error::other)?;
                continue;
            }
            match vref {
                ValueRef::Null => Ok(()), // unbound -> null
                ValueRef::Integer(v) => o.raw_bind_parameter(i + 1, v).map_err(io::Error::other),
                ValueRef::Real(f) => o.raw_bind_parameter(i + 1, f).map_err(io::Error::other),
                ValueRef::Text(t) => o.raw_bind_parameter(i + 1, t).map_err(io::Error::other),
                ValueRef::Blob(b) => o.raw_bind_parameter(i + 1, b).map_err(io::Error::other),
            }?;
        }

        let chg: usize = o.raw_execute().map_err(io::Error::other)?;
        tot += chg;
    }
}

pub struct CompressConfig {
    pub level: i32,
    pub dict: Option<Dictionary>,
    pub size: Option<usize>,
}

impl CompressConfig {
    pub fn build(self) -> Result<CompressContext, io::Error> {
        match (self.dict, self.size) {
            (Some(d), Some(s)) => {
                CompressContext::with_dict_for_size(self.level, d, s).map_err(io::Error::other)
            }
            (Some(d), None) => CompressContext::with_dict(self.level, d).map_err(io::Error::other),
            _ => CompressContext::new(self.level).map_err(io::Error::other),
        }
    }
}

impl Default for CompressConfig {
    fn default() -> Self {
        Self {
            level: 1,
            dict: None,
            size: None,
        }
    }
}

pub const BUFFER_SIZE_DEFAULT: usize = 1048576;

pub struct Config {
    pub create: String,
    pub insert: String,
    pub select: String,
    pub blob_column_index: usize,
    pub buffer_size: usize,
    pub compress: CompressConfig,
}

impl Config {
    pub fn blobs2zstd(self, i: &Connection, mut o: Connection) -> Result<usize, io::Error> {
        o.execute(&self.create, ()).map_err(io::Error::other)?;

        let mut conv: CompressContext = self.compress.build()?;
        let t: Transaction = o.transaction().map_err(io::Error::other)?;
        let mut ostmt: Statement = t.prepare(&self.insert).map_err(io::Error::other)?;
        let mut istmt: Statement = i.prepare(&self.select).map_err(io::Error::other)?;
        let mut buf: Vec<u8> = Vec::with_capacity(self.buffer_size);
        let tot: usize = blobs2zstd(
            &mut conv,
            self.blob_column_index,
            &mut istmt,
            &mut ostmt,
            &mut buf,
        )?;
        istmt.finalize().map_err(io::Error::other)?;
        ostmt.finalize().map_err(io::Error::other)?;
        t.commit().map_err(io::Error::other)?;
        o.close().map_err(|p| p.1).map_err(io::Error::other)?;
        Ok(tot)
    }

    pub fn original2compressed<P>(self, i: P, o: P) -> Result<usize, io::Error>
    where
        P: AsRef<Path>,
    {
        let icon: Connection = Connection::open_with_flags(i, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(io::Error::other)?;
        let ocon: Connection = Connection::open(o).map_err(io::Error::other)?;
        self.blobs2zstd(&icon, ocon)
    }
}

#[cfg(test)]
mod tests {
    mod blob2zstd {
        use zrip::CompressContext;

        #[test]
        fn empty() {
            let level: i32 = 1;
            let mut ctx: CompressContext = CompressContext::new(level).unwrap();
            let input: &[u8] = &[];
            let mut buf: Vec<u8> = vec![];
            let compressed: &[u8] = crate::blob2zstd(&mut ctx, input, &mut buf).unwrap();
            assert!(!compressed.is_empty());
            let decoded: Vec<u8> = zrip::decompress(compressed).unwrap();
            assert!(decoded.is_empty());
        }

        #[test]
        fn simple() {
            let level: i32 = 1;
            let mut ctx: CompressContext = CompressContext::new(level).unwrap();
            let input: &[u8] = b"hello, world";
            let mut buf: Vec<u8> = vec![0; 1024];
            let compressed: &[u8] = crate::blob2zstd(&mut ctx, input, &mut buf).unwrap();
            assert!(!compressed.is_empty());
            let decoded: Vec<u8> = zrip::decompress(compressed).unwrap();
            assert!(!decoded.is_empty());
            assert_eq!(decoded.len(), input.len());
            assert_eq!(decoded, b"hello, world");
        }
    }
}
