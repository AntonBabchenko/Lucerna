//! Zip archive operations for world backups. `zip_dir` writes a
//! folder into a `.zip` file; `extract_zip` extracts a `.zip` into
//! a target folder with zip-slip defense (entries that escape the
//! target via `..` are rejected). Implementation in Task 3.
