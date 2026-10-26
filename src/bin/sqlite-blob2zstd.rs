use std::io;
use std::process::ExitCode;

use io::Read;

use serde_json::Map;
use serde_json::Value;

use rs_sqlite_blob2zstd::CompressConfig;
use rs_sqlite_blob2zstd::Config;

fn io_envkey2val(key: &'static str) -> impl Fn() -> Result<String, io::Error> {
    move || std::env::var(key).map_err(io::Error::other)
}

fn io_printenv(key: &'static str) -> impl FnMut() -> Result<(), io::Error> {
    move || {
        let val: String = io_envkey2val(key)().unwrap_or_default();
        eprintln!("{key}: {val}");
        Ok(())
    }
}

fn string2usize(s: String) -> Result<usize, io::Error> {
    str::parse(&s).map_err(io::Error::other)
}

fn string2u64(s: String) -> Result<u64, io::Error> {
    str::parse(&s).map_err(io::Error::other)
}

fn io_bind<F, G, T, U, E>(mut iofn: F, pure: G) -> impl FnMut() -> Result<U, E>
where
    F: FnMut() -> Result<T, E>,
    G: Fn(T) -> Result<U, E>,
{
    move || {
        let t: T = iofn()?;
        pure(t)
    }
}

fn io_envkey2usize(key: &'static str) -> impl FnMut() -> Result<usize, io::Error> {
    io_bind(io_envkey2val(key), string2usize)
}

fn io_envkey2u64(key: &'static str) -> impl FnMut() -> Result<u64, io::Error> {
    io_bind(io_envkey2val(key), string2u64)
}

fn io_blob_column_index() -> impl FnMut() -> Result<usize, io::Error> {
    io_envkey2usize("ENV_BLOB_COLUMN_INDEX")
}

fn io_buffer_size() -> impl FnMut() -> Result<usize, io::Error> {
    io_envkey2usize("ENV_BUFFER_SIZE")
}

fn io_sql_max() -> impl FnMut() -> Result<u64, io::Error> {
    io_envkey2u64("ENV_SQL_SIZE_MAX")
}

fn bytes2value(b: Vec<u8>) -> Result<Value, io::Error> {
    serde_json::from_slice(&b).map_err(io::Error::other)
}

const SQL_SIZE_MAX_DEFAULT: u64 = 1048576;

fn io_stdin2bytes() -> impl FnMut() -> Result<Vec<u8>, io::Error> {
    || {
        let i = io::stdin().lock();
        let lmt: u64 = io_sql_max()().unwrap_or(SQL_SIZE_MAX_DEFAULT);
        let mut taken = i.take(lmt);
        let mut buf: Vec<u8> = vec![];
        taken.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

fn io_sql_value() -> impl FnMut() -> Result<Value, io::Error> {
    io_bind(io_stdin2bytes(), bytes2value)
}

fn val2string(val: Value) -> Result<String, io::Error> {
    match val {
        Value::String(s) => Ok(s),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected string",
        )),
    }
}

fn map2value(m: &mut Map<String, Value>, key: &str) -> Result<Value, io::Error> {
    let oval: Option<Value> = m.remove(key);
    oval.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("query {key} missing")))
}

fn map2string(m: &mut Map<String, Value>, key: &str) -> Result<String, io::Error> {
    let val: Value = map2value(m, key)?;
    val2string(val)
}

fn val2sql(val: Value) -> Result<(String, String, String), io::Error> {
    let Value::Object(mut m) = val else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "object expected",
        ));
    };

    let create: String = map2string(&mut m, "create")?;
    let insert: String = map2string(&mut m, "insert")?;
    let select: String = map2string(&mut m, "select")?;

    Ok((create, insert, select))
}

fn io_sql_cis() -> impl FnMut() -> Result<(String, String, String), io::Error> {
    io_bind(io_sql_value(), val2sql)
}

fn io_zstd_cfg() -> impl FnMut() -> Result<CompressConfig, io::Error> {
    || Ok(CompressConfig::default())
}

fn io_config() -> impl FnMut() -> Result<Config, io::Error> {
    || {
        let (create, insert, select) = io_sql_cis()()?;
        let blob_column_index: usize = io_blob_column_index()()?;
        let buffer_size: usize = io_buffer_size()()?;
        let compress: CompressConfig = io_zstd_cfg()()?;
        Ok(Config {
            create,
            insert,
            select,
            blob_column_index,
            buffer_size,
            compress,
        })
    }
}

fn io_path2input() -> impl FnMut() -> Result<String, io::Error> {
    io_envkey2val("ENV_IN_SQLITE_DB_PATH")
}

fn io_path2output() -> impl FnMut() -> Result<String, io::Error> {
    io_envkey2val("ENV_OUT_SQLITE_DB_PATH")
}

fn io_print_changed() -> impl FnMut(usize) -> Result<(), io::Error> {
    |chg: usize| {
        println!("changed: {chg}");
        Ok(())
    }
}

fn io_main() -> impl FnOnce() -> Result<(), io::Error> {
    || {
        let cfg: Config = io_config()()?;
        let i: String = io_path2input()()?;
        let o: String = io_path2output()()?;
        let chg: usize = cfg.original2compressed(i, o)?;
        io_print_changed()(chg)
    }
}

fn sub() -> Result<(), io::Error> {
    io_main()()
}

fn main() -> ExitCode {
    sub().map(|_| ExitCode::SUCCESS).unwrap_or_else(|e| {
        eprintln!("{e}");

        io_printenv("ENV_BLOB_COLUMN_INDEX")().unwrap_or_default();
        io_printenv("ENV_BUFFER_SIZE")().unwrap_or_default();
        io_printenv("ENV_SQL_SIZE_MAX")().unwrap_or_default();
        io_printenv("ENV_IN_SQLITE_DB_PATH")().unwrap_or_default();
        io_printenv("ENV_OUT_SQLITE_DB_PATH")().unwrap_or_default();

        ExitCode::FAILURE
    })
}
