# Why scans are fast

Since 0.2.1, a scan went from minutes to a fraction of a second. This document shows
where the time used to go (measured with a profiler, not guessed), what changed, and how
to reproduce the numbers.

## Results

Same machine (Apple M5 Pro, macOS 27.2, APFS, no Time Machine destination configured),
same synthetic tree: 18,073 folders to walk, 120 projects each with a build folder
(`node_modules`, `target`, `.venv`, `vendor`), the 18 rules of a real config, 4 worker
threads.

| Run | 0.2.1 | Now | Factor |
|---|---|---|---|
| First run: 120 folders to exclude | 335 s | 0.16 s | ~2,000× |
| Repeat run: all 120 already excluded | 2.96 s | 0.15 s | ~20× |

On a real home folder (about 23,000 folders, 77 excluded folders, 3.5 GB) a scan takes
0.4 s, measuring sizes included, once the size cache exists. Without the cache, measuring
all 77 folders adds about 0.7 s.

## Where the time went

### 1. Writing a mark: `tmutil addexclusion` took ~11 s per folder

One `tmutil addexclusion <folder>` took 11.0 s. Passing several folders to one call
doesn't help: 10 folders in one call took 110 s. The first run had 120 folders to
exclude with 4 workers, so 120 × 11 s / 4 ≈ 330 s, which matches the measured 335 s.

Why `tmutil` is that slow wasn't investigated. It likely waits on the backup daemon, and
the test machine has no Time Machine destination configured, so it may be faster
elsewhere. It doesn't matter: the new code doesn't run `tmutil` during a scan.

### 2. Checking a mark: `tmutil isexcluded` took ~58 ms per folder

Every candidate folder was checked by starting a `tmutil isexcluded` process and reading
its output: 20 sequential calls took 1.16 s.

A profile of a repeat run (`sample <pid> 2 1`: 1 ms interval for 2 s) shows the 4 worker
threads spending 99.8% of their samples (6,737 of 6,752) in
`is_excluded_from_timemachine` → `Command::output`, waiting for the child's output. With
120 candidates, that's 120 × ~56 ms / 4 threads ≈ 1.7 s of the 2.96 s. Outside of that,
the workers had a handful of samples (`read_dir`, `readdir`, one glob compile). The main
thread was asleep in its 100 ms polling loop the whole time.

### 3. What looked expensive but wasn't

- **Glob compilation.** The old code compiled every rule's pattern again for every
  directory entry, millions of times per scan. In the profile it shows up in 1 sample
  out of 6,752. It's fixed anyway, but it wasn't the problem.
- **`Vec::remove(0)` on the work queue.** Not visible in the profile.
- **The walk.** `readdir` plus a `stat` per entry: small next to the subprocesses.

## What the new code does

1. **Reads the mark in-process.** The sticky exclusion is an extended attribute,
   `com.apple.metadata:com_apple_backup_excludeItem`. `tm::has_exclusion_mark` calls
   `getxattr(2)` with a null buffer, which only asks whether it exists. That takes
   microseconds and starts no process.

2. **Writes the mark in-process.** `tm::set_exclusion_mark` calls CoreServices'
   `CSBackupSetItemExcluded(url, true, false)`. It is the API `tmutil addexclusion`
   (without `-p`) and cargo (for `target/`) use, it's not deprecated in the macOS SDK
   (`BackupCore.h`, available since 10.5), and it took 0.04–0.8 ms per folder. The
   attribute it writes is byte-for-byte the one `tmutil` writes (compared with
   `xattr -px`):

   ```
   62 70 6C 69 73 74 30 30 5F 10 11 63 6F 6D 2E 61   bplist00_..com.a
   70 70 6C 65 2E 62 61 63 6B 75 70 64 08 ...         pple.backupd...
   ```

   So Time Machine, and tools that find excluded folders through Spotlight, see no
   difference. After writing, the code reads the attribute back to confirm it.

3. **No subprocess during a scan.** `tmutil` is now only used by `list`, `exclude` and
   `include` (for `tmutil isexcluded`, which also knows about system-wide exclusions).

4. **A cheaper walk.** `DirEntry::file_type()` comes from `readdir`'s `d_type`, so there
   is no `stat` per entry and symlinks are never followed. On the same tree, asimeow on
   one thread takes 0.28 s; `find tree … -prune -o -type d -print`, which stats every
   entry, takes 0.62 s. Rules are compiled once, and literal file names (most rules) are
   a hash lookup.

5. **No polling.** The queue and its count of pending folders share one mutex and a
   condition variable. Idle workers sleep until there is work or the walk is done, and
   the walk ends exactly when the pending count reaches zero. (This also fixed a race in
   which a worker could end the walk early.)

6. **Threads stop helping at 4.** With 4 threads the walk takes 0.15 s and with 8 it
   still takes 0.15 s: the filesystem is the limit, not the CPU.

7. **Sizes are cached.** Measuring every excluded folder (sum of file sizes) is the only
   expensive part left. The cache in `~/Library/Caches/asimeow/state.json` keeps each
   folder's size with its inode and mtime in nanoseconds. A size is reused while both
   match and it's less than 7 days old. The 7-day limit exists because a folder's mtime
   only changes when its direct entries change, not when files deeper inside grow.

## Semantics that changed with the speedup

`tmutil isexcluded` says a folder is excluded if it has the mark, if a parent folder has
it, or if a system rule covers it (such as `/tmp`). `has_exclusion_mark` only looks at
the folder's own mark. On the first run after upgrading, asimeow may therefore mark a
few folders that were already covered through a parent or a system location. That's
harmless: the result for Time Machine is the same, and every folder that matches a rule
now carries its own mark.

## Reproducing

Generate a test tree. It must not live under `/tmp` or inside a cargo `target/` folder:
both are already excluded, and the old binary would then never write anything.

```python
# gen_tree.py <root>: ~18k folders to walk, 120 projects with a build folder each
import os, sys
root = sys.argv[1]
markers = [("package.json", "node_modules"), ("Cargo.toml", "target"),
           ("pyproject.toml", ".venv"), ("go.mod", "vendor")]

def files(d, n=8):
    for i in range(n):
        open(os.path.join(d, f"f{i}.txt"), "w").close()

def tree(base, depth, fan):
    if depth == 0:
        return
    for i in range(fan):
        d = os.path.join(base, f"d{i}")
        os.makedirs(d, exist_ok=True)
        files(d)
        tree(d, depth - 1, fan)

for p in range(120):
    marker, build = markers[p % 4]
    proj = os.path.join(root, "projects", f"p{p}")
    os.makedirs(proj, exist_ok=True)
    open(os.path.join(proj, marker), "w").close()
    files(proj)
    tree(os.path.join(proj, "src"), 2, 7)     # walked
    tree(os.path.join(proj, build), 2, 10)    # excluded, not walked
    tree(os.path.join(proj, ".git"), 2, 7)    # ignored
tree(os.path.join(root, "docs"), 4, 10)       # walked
```

Point a config's `roots` at it, then:

```bash
# first run: remove the marks, then time a scan
xattr -d com.apple.metadata:com_apple_backup_excludeItem tree/projects/*/{node_modules,target,.venv,vendor}
time asimeow -c config.yaml

# repeat run: time it again (everything is marked now)
time asimeow -c config.yaml

# profile a running scan (built-in macOS sampler)
asimeow -c config.yaml > /dev/null & sample $! 2 1 -file profile.txt
```

Note that a non-dry run saves the size cache (`~/Library/Caches/asimeow/state.json`),
which then holds the test tree instead of your real folders until the next real run.
Use `--dry-run` for repeated timings of the new version.
