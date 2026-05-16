from __future__ import annotations

import argparse
import json
import sqlite3
import string
import time
from pathlib import Path
from typing import Iterator


CJK_PUNCT_RANGES = (
    (0x3000, 0x303F),
    (0xFE10, 0xFE1F),
    (0xFE30, 0xFE4F),
    (0xFF00, 0xFF0F),
    (0xFF1A, 0xFF20),
    (0xFF3B, 0xFF40),
    (0xFF5B, 0xFF65),
)
ASCII_PUNCT = set(string.punctuation)
CLAUSE_LENGTHS = {5, 7}


def is_cjk_punctuation(ch: str) -> bool:
    codepoint = ord(ch)
    return any(start <= codepoint <= end for start, end in CJK_PUNCT_RANGES)


def normalize_text(text: str) -> str:
    text = text.replace("&nbsp;", "").replace("&nbsp", "")
    return "".join(
        ch
        for ch in text
        if ch not in {"\u3000", "\u00a0"}
        and not ch.isspace()
        and ch not in ASCII_PUNCT
        and not is_cjk_punctuation(ch)
    )


def split_poem_clauses(text: str) -> Iterator[str]:
    buffer: list[str] = []
    for ch in text:
        if ch.isspace() or ch in ASCII_PUNCT or is_cjk_punctuation(ch):
            clause = normalize_text("".join(buffer))
            if clause:
                yield clause
            buffer.clear()
        else:
            buffer.append(ch)
    clause = normalize_text("".join(buffer))
    if clause:
        yield clause


def parse_paragraphs(content: str) -> tuple[str | None, str | None, list[str]]:
    if not content.strip():
        return None, None, []

    try:
        value = json.loads(content)
    except json.JSONDecodeError:
        return None, None, [line.strip() for line in content.splitlines() if line.strip()]

    if isinstance(value, dict):
        title = value.get("title") if isinstance(value.get("title"), str) else None
        author = value.get("author") if isinstance(value.get("author"), str) else None
        paragraphs = value.get("content", value.get("paragraphs", []))
        if isinstance(paragraphs, str):
            return title, author, [paragraphs]
        if isinstance(paragraphs, list):
            return title, author, [str(item) for item in paragraphs if str(item).strip()]
        return title, author, []

    if isinstance(value, list):
        return None, None, [str(item) for item in value if str(item).strip()]

    return None, None, [str(value)]


def iter_clause_rows(source: sqlite3.Connection) -> Iterator[tuple[str, int, int, str]]:
    rows = source.execute(
        "SELECT id, title, author, content, dynasty, source FROM poems ORDER BY id"
    )
    seen: set[tuple[int, str]] = set()
    for poem_id, _title, _author, content, _dynasty, _source_name in rows:
        _parsed_title, _parsed_author, paragraphs = parse_paragraphs(content or "")
        for paragraph in paragraphs:
            for clause in split_poem_clauses(paragraph):
                clause_len = len(clause)
                if clause_len not in CLAUSE_LENGTHS:
                    continue
                dedupe_key = (poem_id, clause)
                if dedupe_key in seen:
                    continue
                seen.add(dedupe_key)
                key = "".join(sorted(clause))
                yield (key, clause_len, poem_id, clause)


def connect_source(path: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    connection.execute("PRAGMA query_only = ON")
    connection.execute("PRAGMA mmap_size = 268435456")
    connection.execute("PRAGMA cache_size = -131072")
    return connection


def create_target(path: Path) -> sqlite3.Connection:
    if path.exists():
        path.unlink()
    path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(path)
    connection.execute("PRAGMA journal_mode = OFF")
    connection.execute("PRAGMA synchronous = OFF")
    connection.execute("PRAGMA temp_store = MEMORY")
    connection.execute("PRAGMA cache_size = -262144")
    connection.executescript(
        """
        CREATE TABLE clause_key_index (
            key TEXT NOT NULL,
            len INTEGER NOT NULL,
            poem_id INTEGER NOT NULL,
            clause TEXT NOT NULL,
            PRIMARY KEY (key, len, poem_id, clause)
        ) WITHOUT ROWID;
        """
    )
    return connection


def build_index(source_path: Path, target_path: Path, batch_size: int) -> None:
    start = time.perf_counter()
    source = connect_source(source_path)
    target = create_target(target_path)

    insert_sql = """
        INSERT OR IGNORE INTO clause_key_index
            (key, len, poem_id, clause)
        VALUES (?, ?, ?, ?)
    """
    batch: list[tuple[str, int, int, str]] = []
    total = 0
    with target:
        for row in iter_clause_rows(source):
            batch.append(row)
            if len(batch) >= batch_size:
                target.executemany(insert_sql, batch)
                total += len(batch)
                if total % (batch_size * 20) == 0:
                    elapsed = time.perf_counter() - start
                    print(f"inserted {total:,} clauses in {elapsed:.1f}s", flush=True)
                batch.clear()
        if batch:
            target.executemany(insert_sql, batch)
            total += len(batch)

    target.execute("ANALYZE")
    target.execute("PRAGMA optimize")
    row_count = target.execute("SELECT COUNT(*) FROM clause_key_index").fetchone()[0]
    target.close()
    source.close()
    elapsed = time.perf_counter() - start
    print(f"done: {row_count:,} indexed clauses -> {target_path} ({elapsed:.1f}s)")


def main() -> None:
    parser = argparse.ArgumentParser(description="Build fast 5/7-char poetry clause key index.")
    parser.add_argument("--source", type=Path, default=Path("data/poetry.db"))
    parser.add_argument("--target", type=Path, default=Path("data/poetry_clause_index.db"))
    parser.add_argument("--batch-size", type=int, default=20_000)
    args = parser.parse_args()

    build_index(args.source.resolve(), args.target.resolve(), args.batch_size)


if __name__ == "__main__":
    main()
