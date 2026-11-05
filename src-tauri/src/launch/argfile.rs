//! The version profile's JVM arguments in a Java argument file (`java @file`).
//!
//! Windows caps a command line at 32 767 characters, and a profile's classpath
//! grows with every library times the length of the data folder path: a 26.x
//! loader profile crosses the limit from a data root of about 110 characters,
//! and `CreateProcessW` then refuses with os error 206. The `java` launcher
//! reads arguments from a file since JDK 9; Java 8 reads `@file` as a main
//! class. So on Windows with Java 9 or newer the profile's JVM arguments go in
//! a file, and everything else stays on the command line: the user's own JVM
//! arguments in front of it, the main class and the game arguments (which carry
//! the access token) after it. Spec 2026-11-04 `launch-argfile-design`.
//!
//! The file format is the JDK's own tokenizer (JDK 21u
//! `src/java.base/share/native/libjli/args.c`, `nextToken`), not its manual:
//! a backslash escapes only inside quotes, both `"` and `'` quote, `#` starts a
//! comment even mid-token, and the file is read in text mode. [`encode`] quotes
//! only the characters that need it and writes no escape and no comment at
//! all:
//! - every backslash stays outside quotes, where it is literal, so a
//!   double-byte code page whose trail byte is 0x5C (Shift-JIS, GBK, Big5) is
//!   never read as an escape;
//! - older launchers mishandle escapes (JDK 9 drops the anchor after one;
//!   JDK-8210810, fixed in 11.0.2, loses one split by the 4096-byte read), so
//!   an argument holding a line break, which only an escape could carry, is
//!   refused rather than written;
//! - every argument ends its line, the last one too: before JDK-8231863 a last
//!   token with no newline after it skipped the launcher's main-class tracking.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// How the profile's JVM arguments reach the JVM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JvmArgsRoute {
    /// On the command line, as before.
    CommandLine,
    /// In a Java argument file named `@<path>` on the command line.
    ArgFile,
}

/// The first Java whose launcher reads `@argfiles` (JDK-8027634).
const FIRST_ARGFILE_JAVA: u32 = 9;

/// The route for a launch on `os` with a Java of `java_major`. Only Windows
/// needs a file: Linux allows 128 KiB per argument, macOS 1 MiB in all. A Java
/// whose major is not known is treated as Java 8 — a file on Java 8 would fail
/// every launch, the command line fails only one that is too long. So does a
/// launch where `--disable-@files` is set ([`expansion_disabled`]): the
/// launcher would take `@<file>` for the main class.
pub fn route_for(os: &str, java_major: Option<u32>, expansion_disabled: bool) -> JvmArgsRoute {
    match java_major {
        Some(major) if os == "windows" && major >= FIRST_ARGFILE_JAVA && !expansion_disabled => {
            JvmArgsRoute::ArgFile
        }
        _ => JvmArgsRoute::CommandLine,
    }
}

/// The launcher option that turns `@file` expansion off for everything after it.
pub const DISABLE_ARGFILES: &str = "--disable-@files";

/// Whether this launch turns argument files off: the instance's own JVM
/// arguments (which come before `@<file>`), or `JDK_JAVA_OPTIONS` inherited
/// from the environment, which the launcher reads before its command line
/// (and where it takes quotes off a token, so a quoted one counts too).
pub fn expansion_disabled(user_jvm: &[String], jdk_java_options: Option<&str>) -> bool {
    let unquoted = |token: &str| token.trim_matches(|c| c == '"' || c == '\'') == DISABLE_ARGFILES;
    user_jvm.iter().any(|a| a == DISABLE_ARGFILES)
        || jdk_java_options.is_some_and(|v| v.split_whitespace().any(unquoted))
}

/// Why an argument cannot travel in an argument file with its meaning intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgfileRefusal {
    /// NUL or U+001A: the launcher reads the file in text mode, where U+001A
    /// ends it, and a NUL ends the C string it builds.
    Unreadable { index: usize },
    /// LF or CR: ends the argument even inside quotes, and the escape that
    /// could carry it is broken in older launchers.
    LineBreak { index: usize },
    /// A leading `@` names an argument file on the command line, but inside a
    /// file it is taken as the main class (argument files do not nest).
    AtSign { index: usize },
}

impl std::fmt::Display for ArgfileRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable { index } => write!(
                f,
                "JVM argument {index} holds a NUL or U+001A character, which an argument file cannot carry"
            ),
            Self::LineBreak { index } => write!(
                f,
                "JVM argument {index} holds a line break, which an argument file cannot carry"
            ),
            Self::AtSign { index } => write!(
                f,
                "JVM argument {index} starts with '@', which an argument file would read as the main class"
            ),
        }
    }
}

/// One argument as the JDK tokenizer must read it back. Unquoted text is
/// literal there, backslashes included; a run of characters that would split
/// the token or start a comment (space, TAB, FF, `#`, `'`) goes in double
/// quotes, where each is literal; a run of `"` goes in single quotes. An empty
/// argument is `""`. `encode` has already refused what no quoting can carry.
fn encode_one(arg: &str, out: &mut String) {
    #[derive(PartialEq)]
    enum Quote {
        None,
        Double,
        Single,
    }
    let close = |q: &Quote, out: &mut String| match q {
        Quote::None => {}
        Quote::Double => out.push('"'),
        Quote::Single => out.push('\''),
    };
    if arg.is_empty() {
        out.push_str("\"\"");
        return;
    }
    let mut open = Quote::None;
    for c in arg.chars() {
        let want = match c {
            ' ' | '\t' | '\u{c}' | '#' | '\'' => Quote::Double,
            '"' => Quote::Single,
            _ => Quote::None,
        };
        if want != open {
            close(&open, out);
            match want {
                Quote::None => {}
                Quote::Double => out.push('"'),
                Quote::Single => out.push('\''),
            }
            open = want;
        }
        out.push(c);
    }
    close(&open, out);
}

/// The argument file's text: one argument per line, each read back by the JDK
/// tokenizer exactly as given, or the first argument that cannot be.
pub fn encode(args: &[String]) -> std::result::Result<String, ArgfileRefusal> {
    let mut out = String::new();
    for (index, arg) in args.iter().enumerate() {
        if arg.contains(['\0', '\u{1a}']) {
            return Err(ArgfileRefusal::Unreadable { index });
        }
        if arg.contains(['\n', '\r']) {
            return Err(ArgfileRefusal::LineBreak { index });
        }
        if arg.starts_with('@') {
            return Err(ArgfileRefusal::AtSign { index });
        }
        encode_one(arg, &mut out);
        out.push('\n');
    }
    Ok(out)
}

/// Write `args` to `path` as an argument file in the bytes the launcher reads
/// (`platform::encoding::jvm_launcher_bytes`), replacing any earlier one.
/// Returns the number of bytes written. When an argument cannot be carried
/// (the refusal names it) or the write fails, the launch stops there, and no
/// file is left behind — neither an earlier launch's nor a partial one.
pub fn write(path: &Path, args: &[String]) -> Result<u64> {
    let written = file_bytes(args).and_then(|bytes| {
        std::fs::write(path, &bytes)
            .map(|()| bytes.len() as u64)
            .map_err(|e| Error::io(path.display().to_string(), e))
    });
    if written.is_err() {
        remove(path);
    }
    written
}

/// The argument file's bytes, or the refusal naming the argument that cannot
/// be carried.
fn file_bytes(args: &[String]) -> Result<Vec<u8>> {
    let refused = |details: String| Error::JavaSpawn {
        details: format!("cannot write the JVM argument file: {details}"),
    };
    let text = encode(args).map_err(|r| refused(r.to_string()))?;
    let mut bytes = Vec::with_capacity(text.len());
    // Line by line, so a refusal can name the argument (one line each).
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let encoded = crate::platform::encoding::jvm_launcher_bytes(line).map_err(|why| {
            refused(format!(
                "JVM argument {index} cannot be passed to Java on this system: {why} ({})",
                args.get(index).map(|arg| excerpt(arg)).unwrap_or_default()
            ))
        })?;
        bytes.extend_from_slice(&encoded);
    }
    Ok(bytes)
}

/// Remove the argument file, so that one is there only when the instance's
/// last launch read it: after a launch on the command line, a write that
/// failed, or a spawn that failed. Not after the game exits — a mod that restarts the game re-runs its
/// command line, `@<file>` included. Already gone is fine; any other failure is
/// logged: the file holds paths and flags, and the next launch replaces it.
pub fn remove(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => crate::diag!(
            "launch: could not remove the JVM argument file {}: {e}",
            path.display()
        ),
    }
}

/// The first 120 characters of an argument, for an error message.
fn excerpt(arg: &str) -> String {
    const MAX: usize = 120;
    match arg.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &arg[..cut]),
        None => arg.to_string(),
    }
}

/// Where the profile's JVM arguments sit on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileJvm {
    Inline(Vec<String>),
    File(PathBuf),
}

/// The argv after `java`: the user's JVM arguments (heap, then the extra ones),
/// the profile's JVM arguments — inline or as `@<file>`, which the launcher
/// expands in place — then the main class and the game arguments. The user's
/// come first so a profile flag can still override one.
pub fn command_line(
    user_jvm: Vec<String>,
    profile: ProfileJvm,
    main_class: String,
    game: Vec<String>,
) -> Vec<String> {
    let mut argv = user_jvm;
    match profile {
        ProfileJvm::Inline(args) => argv.extend(args),
        ProfileJvm::File(path) => argv.push(format!("@{}", path.to_string_lossy())),
    }
    argv.push(main_class);
    argv.extend(game);
    argv
}

#[cfg(test)]
mod jdk_tokenizer {
    //! A line-by-line port of the reader `java @file` uses: JDK 21u
    //! `src/java.base/share/native/libjli/args.c`, `nextToken` and
    //! `readArgFile` (4096-byte reads included), so the tests can read back
    //! what [`super::encode`] wrote. Kept faithful, quirks too: a `#` outside
    //! quotes drops the token it interrupts.

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        FindNext,
        InComment,
        InQuote,
        InEscape,
        SkipLeadWs,
        InToken,
    }

    struct Ctx {
        state: State,
        parts: Vec<Vec<u8>>,
        quote: u8,
    }

    fn is_ws(c: u8) -> bool {
        matches!(c, b' ' | b'\n' | b'\r' | b'\t' | 0x0c)
    }

    fn next_token(ctx: &mut Ctx, buf: &[u8], cptr: &mut usize) -> Option<Vec<u8>> {
        let eob = buf.len();
        let mut nextc = *cptr;
        let mut anchor = nextc;
        while nextc < eob {
            let mut ch = buf[nextc];
            if matches!(ctx.state, State::FindNext | State::SkipLeadWs) {
                while is_ws(ch) {
                    nextc += 1;
                    if nextc >= eob {
                        return None;
                    }
                    ch = buf[nextc];
                }
                ctx.state = if ctx.state == State::FindNext {
                    State::InToken
                } else {
                    State::InQuote
                };
                anchor = nextc;
            } else if ctx.state == State::InEscape {
                if ch == b'\n' || ch == b'\r' {
                    ctx.state = State::SkipLeadWs;
                } else {
                    let escaped = match ch {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'f' => 0x0c,
                        other => other,
                    };
                    ctx.parts.push(vec![escaped]);
                    ctx.state = State::InQuote;
                }
                anchor = nextc + 1;
                nextc += 1;
                continue;
            } else if ctx.state == State::InComment {
                while ch != b'\n' && ch != b'\r' {
                    nextc += 1;
                    if nextc >= eob {
                        return None;
                    }
                    ch = buf[nextc];
                }
                anchor = nextc + 1;
                ctx.state = State::FindNext;
                nextc += 1;
                continue;
            }

            match ch {
                b' ' | b'\t' | 0x0c if ctx.state == State::InQuote => {}
                b' ' | b'\t' | 0x0c | b'\n' | b'\r' => {
                    let token = if ctx.parts.is_empty() {
                        buf[anchor..nextc].to_vec()
                    } else {
                        ctx.parts.push(buf[anchor..nextc].to_vec());
                        let t = ctx.parts.concat();
                        ctx.parts.clear();
                        t
                    };
                    *cptr = nextc + 1;
                    ctx.state = State::FindNext;
                    return Some(token);
                }
                b'#' => {
                    if ctx.state != State::InQuote {
                        ctx.state = State::InComment;
                        anchor = nextc + 1;
                    }
                }
                b'\\' => {
                    if ctx.state == State::InQuote {
                        ctx.parts.push(buf[anchor..nextc].to_vec());
                        ctx.state = State::InEscape;
                        anchor = nextc + 1;
                    }
                }
                b'\'' | b'"' => {
                    if !(ctx.state == State::InQuote && ctx.quote != ch) {
                        if anchor != nextc {
                            ctx.parts.push(buf[anchor..nextc].to_vec());
                        }
                        anchor = nextc + 1;
                        if ctx.state == State::InToken {
                            ctx.quote = ch;
                            ctx.state = State::InQuote;
                        } else {
                            ctx.state = State::InToken;
                        }
                    }
                }
                _ => {}
            }
            nextc += 1;
        }
        if matches!(ctx.state, State::InToken | State::InQuote) && anchor < nextc {
            ctx.parts.push(buf[anchor..nextc].to_vec());
        }
        None
    }

    /// The arguments `java` reads from a file holding `bytes`.
    pub fn read_arg_file(bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut ctx = Ctx {
            state: State::FindNext,
            parts: Vec::new(),
            quote: b'"',
        };
        let mut out = Vec::new();
        for chunk in bytes.chunks(4096) {
            let mut cptr = 0;
            while let Some(token) = next_token(&mut ctx, chunk, &mut cptr) {
                out.push(token);
            }
        }
        if matches!(ctx.state, State::InToken | State::InQuote) && !ctx.parts.is_empty() {
            out.push(ctx.parts.concat());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::jdk_tokenizer::read_arg_file;
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// What makes the file safe beyond the 21u reader, checked on every
    /// encoding a test makes: no backslash inside quotes (the only place it
    /// would be an escape), no `#` outside quotes (a comment), and every
    /// argument ending its line, the last one included.
    fn assert_reader_independent(text: &str, args: &[String]) {
        let mut quote: Option<char> = None;
        for c in text.chars() {
            match (quote, c) {
                (Some(_), '\\') => panic!("a backslash inside quotes:\n{text}"),
                (None, '#') => panic!("an unquoted #:\n{text}"),
                (Some(q), c) if c == q => quote = None,
                (None, '"' | '\'') => quote = Some(c),
                _ => {}
            }
        }
        assert!(quote.is_none(), "an unclosed quote:\n{text}");
        assert_eq!(text.matches('\n').count(), args.len(), "{text}");
        assert!(args.is_empty() || text.ends_with('\n'), "{text}");
    }

    /// Encode, read back with the JDK's reader, and compare.
    fn assert_round_trip(args: &[String]) {
        let text = encode(args).expect("encodable");
        assert_reader_independent(&text, args);
        let back: Vec<String> = read_arg_file(text.as_bytes())
            .into_iter()
            .map(|b| String::from_utf8(b).expect("utf-8 back"))
            .collect();
        assert_eq!(back, args, "the file was:\n{text}");
    }

    // ---- route_for ----------------------------------------------------------

    #[test]
    fn windows_with_java_9_or_newer_reads_the_profile_arguments_from_a_file() {
        for major in [9, 11, 16, 17, 21, 25] {
            assert_eq!(
                route_for("windows", Some(major), false),
                JvmArgsRoute::ArgFile,
                "Java {major}"
            );
        }
    }

    #[test]
    fn java_8_and_an_unknown_java_keep_the_command_line() {
        assert_eq!(
            route_for("windows", Some(8), false),
            JvmArgsRoute::CommandLine
        );
        assert_eq!(route_for("windows", None, false), JvmArgsRoute::CommandLine);
    }

    // With --disable-@files before it, the launcher reads `@<file>` as the main
    // class: every launch would fail.
    #[test]
    fn disabled_argument_files_keep_the_command_line() {
        assert_eq!(
            route_for("windows", Some(21), true),
            JvmArgsRoute::CommandLine
        );
    }

    #[test]
    fn the_instance_args_or_jdk_java_options_can_disable_argument_files() {
        let user = strings(&["-Xmx2048m", "--disable-@files"]);
        assert!(expansion_disabled(&user, None));
        let plain = strings(&["-Xmx2048m", "-XX:+UseG1GC"]);
        assert!(!expansion_disabled(&plain, None));
        assert!(expansion_disabled(&plain, Some("-Dx=1  --disable-@files")));
        assert!(expansion_disabled(
            &plain,
            Some("\"--disable-@files\" -Dx=1")
        ));
        assert!(!expansion_disabled(
            &plain,
            Some("-Dfoo=--disable-@files-not")
        ));
        assert!(!expansion_disabled(&plain, Some("")));
    }

    #[test]
    fn linux_and_macos_keep_the_command_line() {
        for os in ["linux", "macos"] {
            assert_eq!(
                route_for(os, Some(21), false),
                JvmArgsRoute::CommandLine,
                "{os}"
            );
        }
    }

    // ---- encode: read back exactly -------------------------------------------

    #[test]
    fn paths_with_spaces_and_backslashes_read_back_exactly() {
        assert_round_trip(&strings(&[
            r"-Djava.library.path=C:\Users\Анна Мария\Lucerna Data\instances\x\natives",
            "-cp",
            r"C:\Lucerna Data\libraries\a b\lib.jar;C:\Lucerna Data\libraries\c\d.jar",
            "-Dos.name=Windows 10",
            "net.minecraft.client.main.Main",
        ]));
    }

    #[test]
    fn a_trailing_backslash_reads_back_exactly() {
        assert_round_trip(&strings(&[r"C:\dir\", r"\\server\share\", r"\", r"a\ "]));
    }

    #[test]
    fn quotes_hashes_and_apostrophes_read_back_exactly() {
        assert_round_trip(&strings(&[
            r#"-Dx="quoted""#,
            "O'Brien's",
            r"C:\C#\lib.jar",
            "#start",
            r#"mixed '"' and "'""#,
            r#""""#,
        ]));
    }

    #[test]
    fn tabs_form_feeds_and_spaces_read_back_exactly() {
        assert_round_trip(&strings(&[
            "a\tb",
            "ff\u{c}x",
            " leading",
            "trailing ",
            "  ",
            "\t",
        ]));
    }

    #[test]
    fn a_line_break_is_refused() {
        assert_eq!(
            encode(&strings(&["ok", "line1\nline2"])),
            Err(ArgfileRefusal::LineBreak { index: 1 })
        );
        assert_eq!(
            encode(&strings(&["cr\rhere"])),
            Err(ArgfileRefusal::LineBreak { index: 0 })
        );
    }

    #[test]
    fn an_empty_argument_reads_back_as_one() {
        assert_round_trip(&strings(&["", "-Da=", ""]));
    }

    #[test]
    fn a_classpath_longer_than_the_readers_buffer_reads_back_exactly() {
        let entry =
            r"C:\Users\Анна\Very Long Data Folder\libraries\org\ow2\asm\asm\9.8\asm-9.8.jar";
        let cp = vec![entry; 400].join(";");
        assert!(cp.len() > 30_000);
        assert_round_trip(&strings(&["-cp", &cp, "-Dos.name=Windows 10"]));
    }

    #[test]
    fn every_special_sequence_survives_a_4096_byte_read_boundary() {
        // Slide the quoted parts and the backslashes across the reader's chunk edge.
        for pad in 4080..4100 {
            let arg = format!("{} #'\"\t\u{c}\\end\\", "x".repeat(pad));
            assert_round_trip(&[arg, String::new(), "after".into()]);
        }
    }

    // A double-byte code page can put 0x5C (backslash) as the second byte of a
    // character — Shift-JIS 表 is 0x95 0x5C. Inside quotes the reader would
    // take it as an escape, so `encode` must never put such text in quotes.
    #[test]
    fn backslashes_and_other_text_stay_outside_quotes() {
        assert_eq!(
            encode(&strings(&[r"C:\表 示\x"])).unwrap(),
            "C:\\表\" \"示\\x\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_shift_jis_trail_byte_that_is_a_backslash_reads_back_exactly() {
        use crate::platform::encoding::{decode_with_code_page, encode_in_code_page};
        let arg = r"C:\ユーザー\表 示\表\lib.jar".to_string();
        let text = encode(std::slice::from_ref(&arg)).unwrap();
        let bytes = encode_in_code_page(&text, 932).expect("932 holds it");
        assert!(bytes.windows(2).any(|w| w == [0x95, 0x5c]), "{bytes:x?}");
        let back = read_arg_file(&bytes);
        assert_eq!(back.len(), 1, "{back:x?}");
        assert_eq!(decode_with_code_page(&back[0], 932), arg);
    }

    #[test]
    fn nul_and_u001a_are_refused() {
        assert_eq!(
            encode(&strings(&["ok", "a\0b"])),
            Err(ArgfileRefusal::Unreadable { index: 1 })
        );
        assert_eq!(
            encode(&strings(&["a\u{1a}b"])),
            Err(ArgfileRefusal::Unreadable { index: 0 })
        );
    }

    #[test]
    fn a_leading_at_sign_is_refused() {
        assert_eq!(
            encode(&strings(&["-Dok=1", "@more.txt"])),
            Err(ArgfileRefusal::AtSign { index: 1 })
        );
        // Only a LEADING one changes meaning.
        assert!(encode(&strings(&["-Dmail=a@b"])).is_ok());
    }

    // ---- command_line -------------------------------------------------------

    fn user() -> Vec<String> {
        strings(&["-Xmx4096m", "-XX:+UseG1GC"])
    }

    fn game() -> Vec<String> {
        strings(&["--username", "Steve", "--accessToken", "secret"])
    }

    #[test]
    fn inline_profile_arguments_keep_the_order_launches_had() {
        let argv = command_line(
            user(),
            ProfileJvm::Inline(strings(&["-cp", "a.jar"])),
            "net.Main".into(),
            game(),
        );
        assert_eq!(
            argv,
            strings(&[
                "-Xmx4096m",
                "-XX:+UseG1GC",
                "-cp",
                "a.jar",
                "net.Main",
                "--username",
                "Steve",
                "--accessToken",
                "secret"
            ])
        );
    }

    #[test]
    fn a_file_takes_the_place_of_the_profile_arguments() {
        let file = PathBuf::from(r"C:\Lucerna Data\instances\x\jvm-args.txt");
        let argv = command_line(user(), ProfileJvm::File(file), "net.Main".into(), game());
        assert_eq!(
            argv,
            strings(&[
                "-Xmx4096m",
                "-XX:+UseG1GC",
                r"@C:\Lucerna Data\instances\x\jvm-args.txt",
                "net.Main",
                "--username",
                "Steve",
                "--accessToken",
                "secret"
            ])
        );
    }

    // ---- write / remove ------------------------------------------------------

    #[test]
    fn write_replaces_an_earlier_file_and_counts_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jvm-args.txt");
        write(
            &path,
            &strings(&["-cp", "old one.jar", "-Dlong=xxxxxxxxxxxxxxxx"]),
        )
        .unwrap();
        let n = write(&path, &strings(&["-cp", "a b.jar"])).unwrap();
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(n, on_disk.len() as u64);
        assert_eq!(on_disk, b"-cp\na\" \"b.jar\n");
    }

    #[test]
    fn a_refused_argument_leaves_no_file_not_even_an_earlier_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jvm-args.txt");
        write(&path, &strings(&["-cp", "earlier.jar"])).unwrap();
        let r = write(&path, &strings(&["-cp", "@x"]));
        assert!(matches!(r, Err(Error::JavaSpawn { .. })), "{r:?}");
        assert!(
            !path.exists(),
            "an earlier launch's file must not pass for this one's"
        );
    }

    #[test]
    fn remove_takes_the_file_and_is_quiet_when_it_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jvm-args.txt");
        write(&path, &strings(&["-cp", "a.jar"])).unwrap();
        remove(&path);
        assert!(!path.exists());
        remove(&path);
    }
}
