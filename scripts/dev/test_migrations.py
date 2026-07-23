"""Apply all migrations (1..N) against a fresh in-memory DB to verify
the latest set still composes cleanly. Mirrors what cargo's
apply_all_brings_fresh_db_to_version_N test does, but works around
the pre-existing STATUS_ENTRYPOINT_NOT_FOUND DLL-load issue in the
Rust test runner. Useful for iterating on migrations without
re-running the full app each time."""
import sqlite3, sys, os, glob, re

mig_dir = r'src-tauri\src\db\migrations'
prompts_dir = r'src-tauri\src\cleanup\prompts'

# Map __PROMPT_<NAME>_BODY__ token -> source file (mirrors prompt_loader.rs).
PROMPT_FILES = {
    '__PROMPT_NORMAL_BODY__':            'normal.md',
    '__PROMPT_NORMAL_V2_BODY__':         'normal_v2.md',
    '__PROMPT_NORMAL_V3_BODY__':         'normal_v3.md',
    '__PROMPT_NORMAL_V4_BODY__':         'normal_v4.md',
    '__PROMPT_NORMAL_V5_BODY__':         'normal_v5.md',
    '__PROMPT_NORMAL_V6_ADDITIVE_BODY__':'normal_v6_additive.md',
    '__PROMPT_NORMAL_SMALL_BODY__':      'normal_small.md',
    '__PROMPT_CASUAL_V1_BODY__':         'casual_v1.md',
    '__PROMPT_CASUAL_V2_BODY__':         'casual_v2.md',
    '__PROMPT_FORMAL_V1_BODY__':         'formal_v1.md',
    '__PROMPT_FORMAL_V2_BODY__':         'formal_v2.md',
    '__PROMPT_VERBOSE_BODY__':           'verbose.md',
    '__PROMPT_FRAGMENT_BODY__':          'fragment.md',
    '__PROMPT_REWRITE_BODY__':           'rewrite.md',
    '__PROMPT_EXPAND_BODY__':            'expand.md',
    '__PROMPT_SUMMARIZE_BODY__':         'summarize.md',
}

def load_body(token):
    path = os.path.join(prompts_dir, PROMPT_FILES[token])
    with open(path, 'r', encoding='utf-8') as f:
        return f.read()

def substitute(sql):
    for tok in PROMPT_FILES:
        sql = sql.replace(tok, load_body(tok).replace("'", "''"))
    # Mimic the Rust leftover-token guard.
    leftover = re.search(r'__PROMPT_[A-Z0-9_]+_BODY__', sql)
    if leftover:
        raise RuntimeError(f"unsubstituted token: {leftover.group(0)}")
    return sql

mig_files = sorted(glob.glob(os.path.join(mig_dir, '*.sql')))
print(f'Found {len(mig_files)} migrations', flush=True)

conn = sqlite3.connect(':memory:')
conn.execute('PRAGMA foreign_keys = ON;')
for path in mig_files:
    with open(path, 'r', encoding='utf-8') as f:
        sql = f.read()
    sql = substitute(sql)
    try:
        conn.executescript(sql)
        print(f'  OK {os.path.basename(path)}', flush=True)
    except Exception as e:
        print(f'  FAIL {os.path.basename(path)}: {type(e).__name__}: {e}', flush=True)
        sys.exit(1)

cur = conn.cursor()
print()
print('schema_version:', cur.execute("SELECT value FROM schema_meta WHERE key='schema_version'").fetchone()[0], flush=True)
print('prompts:')
for r in cur.execute('SELECT id, mode_slug, version, length(body) FROM prompts ORDER BY id'):
    print(f'  id={r[0]}  mode={r[1]:10s}  v={r[2]}  body_len={r[3]}', flush=True)
print('modes:')
for r in cur.execute('SELECT id, slug, prompt_id, enabled FROM modes ORDER BY id'):
    print(f'  id={r[0]}  slug={r[1]:10s}  prompt_id={r[2]}  enabled={r[3]}', flush=True)
