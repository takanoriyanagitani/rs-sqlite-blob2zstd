#!/bin/bash

wsm="./sqlite-blob2zstd.wasm"

hdir="$PWD"
gdir="/guest.d"
bname="in.sqlite.db3"
ipath="${hdir}/${bname}"

# id: n/a, unixtime_ms: 0, message: 1
blob_column_index=1
buffer_size=16777216
sqlmax=65536

run_wasi() {
	wasmtime \
		run \
		--env ENV_BLOB_COLUMN_INDEX=$blob_column_index \
		--env ENV_BUFFER_SIZE=$buffer_size \
		--env ENV_SQL_SIZE_MAX=$sqlmax \
		--env ENV_IN_SQLITE_DB_PATH="${gdir}/in.sqlite.db3" \
		--env ENV_OUT_SQLITE_DB_PATH="${gdir}/out.sqlite.db3" \
		--dir "${hdir}::${gdir}" \
		"${wsm}"
}

geninput() {
	echo generating example sqlite file...

	sqlite3 "${ipath}" "
    CREATE TABLE IF NOT EXISTS log (
      id INTEGER PRIMARY KEY,
      unixtime_ms INTEGER NOT NULL,
      message BLOB
    )
  "

	echo "
		INSERT INTO log(unixtime_ms, message) VALUES(0, 'helo' || char(10))
  " | sqlite3 "${ipath}"

	echo "
		INSERT INTO log(unixtime_ms, message) VALUES(1, 'wrld' || char(10))
  " | sqlite3 "${ipath}"

}

test -f "${ipath}" || geninput

echo '
create: |
  CREATE TABLE IF NOT EXISTS log(
      id INTEGER PRIMARY KEY,
      unixtime_ms INTEGER NOT NULL,
      message BLOB
  )
insert: |
  INSERT INTO log(unixtime_ms, message)
  VALUES(?1, ?2)
select: |
  SELECT unixtime_ms, message FROM log
  ORDER BY id
' |
	dasel --in=yaml --out=json |
	run_wasi
