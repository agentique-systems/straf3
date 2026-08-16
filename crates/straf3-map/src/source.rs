//! Making real Quake 3 `.map` text parseable.
//!
//! # Why this exists at all
//!
//! `quake-map` says it plainly in its own docs: *"Quake 3 `brushDef`s/
//! `patchDef`s are not presently supported"*. It parses the Quake 1/2 face
//! syntax with either legacy or Valve 220 texture alignment, which is what most
//! of the Quake 3 corpus is written in — and then hits its first curved surface
//! and fails the **whole file**, because a `patchDef2` block appears where the
//! parser expects a face.
//!
//! That is not a rare shape. Practically every Quake 3 map made in Radiant has
//! at least one patch in it, and maps saved in brush-primitives mode are all
//! `brushDef`. A compiler that rejects them cannot ingest the corpus the
//! operator chose to import, so this module makes the two constructs
//! parseable before `quake-map` sees them:
//!
//! - **`brushDef`** is rewritten into the equivalent legacy brush. It is a pure
//!   syntax change: the three plane points are identical and mean the same
//!   thing (q3map2 hands both formats to the same `PlaneFromPoints`), and what
//!   is dropped is the texture-alignment matrix, which is out of scope this
//!   wave anyway.
//! - **`patchDef2` / `patchDef3`** are parsed only far enough to be skipped —
//!   they are **dropped**, never tessellated — and every one is counted and
//!   reported as [`Warning::PatchDropped`]. This is a real loss and must not be
//!   quiet: a Quake 3 patch is collidable, so a map whose route runs over a
//!   curved ramp will have a hole where the ramp was. The count is what
//!   [`PatchLoss`] grades, because a map that loses an arch and a map that
//!   loses a thousand ramps are different outcomes, not different magnitudes of
//!   one. Tessellating Bézier patches into hulls is a larger piece of work than
//!   the rest of this compiler, is not in C7's five requirements, and is a
//!   Wave 5 question.
//!
//! # Why the untouched path is byte-for-byte untouched
//!
//! A map containing neither construct is handed to `quake-map` exactly as it
//! arrived. That keeps the common case free, and — more usefully — keeps the
//! **line numbers in parse errors true**, because rewriting re-emits the token
//! stream and would report a syntax error on the wrong line.
//!
//! [`Warning::PatchDropped`]: crate::Warning::PatchDropped
//! [`PatchLoss`]: crate::PatchLoss

/// One lexical token of `.map` text.
///
/// Comments are dropped by the lexer, quoted strings are kept whole, and
/// everything else is a bare word — including numbers, which this module never
/// needs to interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    LBrace,
    RBrace,
    LParen,
    RParen,
    Word(String),
    Quoted(String),
}

impl Tok {
    fn write_to(&self, out: &mut String) {
        match self {
            Self::LBrace => out.push('{'),
            Self::RBrace => out.push('}'),
            Self::LParen => out.push('('),
            Self::RParen => out.push(')'),
            Self::Word(w) => out.push_str(w),
            Self::Quoted(q) => {
                out.push('"');
                out.push_str(q);
                out.push('"');
            }
        }
    }
}

/// `.map` text with the Quake 3 constructs `quake-map` cannot read removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    /// What to hand the parser.
    pub text: String,
    /// How many `brushDef` brushes were rewritten.
    pub brush_defs: usize,
    /// How many patches were dropped. Each one is geometry that is gone.
    pub patches_dropped: usize,
}

/// Whether the source contains anything needing a rewrite.
///
/// A substring test, so a `//` comment mentioning `patchDef` sends the file
/// down the slow path unnecessarily. That is the safe direction to be wrong in:
/// the slow path produces the same brushes, it just renumbers the lines a parse
/// error would cite.
fn needs_rewrite(source: &str) -> bool {
    source.contains("brushDef") || source.contains("patchDef")
}

/// Prepare `.map` source for `quake-map`.
pub(crate) fn prepare(source: &str) -> Prepared {
    if !needs_rewrite(source) {
        return Prepared {
            text: source.to_string(),
            brush_defs: 0,
            patches_dropped: 0,
        };
    }

    let tokens = lex(source);
    let mut out = Rewriter::default();
    out.map(&tokens);
    Prepared {
        text: out.text,
        brush_defs: out.brush_defs,
        patches_dropped: out.patches_dropped,
    }
}

/// Split `.map` text into tokens, dropping `//` comments.
fn lex(source: &str) -> Vec<Tok> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                j += 1;
            }
            tokens.push(Tok::Quoted(
                String::from_utf8_lossy(&bytes[start..j.min(bytes.len())]).into_owned(),
            ));
            i = j + 1;
            continue;
        }
        let single = match c {
            b'{' => Some(Tok::LBrace),
            b'}' => Some(Tok::RBrace),
            b'(' => Some(Tok::LParen),
            b')' => Some(Tok::RParen),
            _ => None,
        };
        if let Some(tok) = single {
            tokens.push(tok);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'{' | b'}' | b'(' | b')' | b'"')
        {
            i += 1;
        }
        tokens.push(Tok::Word(
            String::from_utf8_lossy(&bytes[start..i]).into_owned(),
        ));
    }
    tokens
}

/// Walks the token stream and writes the rewritten source.
///
/// Written as a cursor over a slice rather than a recursive-descent parser with
/// error handling, for one reason: a malformed file must still come out the
/// other side. `quake-map` is the parser and its error messages are the ones a
/// mapper should see, so anything this does not understand is copied through
/// verbatim and left for it to complain about.
#[derive(Debug, Default)]
struct Rewriter {
    text: String,
    brush_defs: usize,
    patches_dropped: usize,
}

impl Rewriter {
    fn push(&mut self, tok: &Tok) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push(' ');
        }
        tok.write_to(&mut self.text);
        if matches!(tok, Tok::LBrace | Tok::RBrace) {
            self.text.push('\n');
        }
    }

    fn newline(&mut self) {
        if !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }

    fn map(&mut self, tokens: &[Tok]) {
        let mut i = 0;
        while i < tokens.len() {
            if tokens[i] == Tok::LBrace {
                i = self.entity(tokens, i);
            } else {
                // Stray token outside any entity. Pass it through; the parser
                // will say what it thinks of it.
                self.push(&tokens[i]);
                i += 1;
            }
        }
    }

    /// Copy one entity, rewriting the brushes inside it. Returns the index just
    /// past its closing brace.
    fn entity(&mut self, tokens: &[Tok], start: usize) -> usize {
        self.push(&tokens[start]);
        let mut i = start + 1;
        while i < tokens.len() {
            match &tokens[i] {
                Tok::RBrace => {
                    self.push(&tokens[i]);
                    return i + 1;
                }
                Tok::LBrace => i = self.brush(tokens, i),
                other => {
                    self.push(other);
                    i += 1;
                }
            }
        }
        i
    }

    /// Copy, rewrite or drop one brush. Returns the index just past it.
    fn brush(&mut self, tokens: &[Tok], start: usize) -> usize {
        match tokens.get(start + 1) {
            Some(Tok::Word(w)) if w.starts_with("patchDef") => {
                self.patches_dropped += 1;
                skip_balanced(tokens, start)
            }
            Some(Tok::Word(w)) if w.starts_with("brushDef") => {
                self.brush_defs += 1;
                self.brush_def(tokens, start)
            }
            _ => {
                // An ordinary brush: copy it through token for token, so a file
                // that needed the slow path only because one brush was a patch
                // keeps every other brush exactly as its author wrote it.
                let end = skip_balanced(tokens, start);
                for tok in &tokens[start..end] {
                    self.push(tok);
                }
                end
            }
        }
    }

    /// Rewrite a `brushDef` brush into the legacy face syntax.
    ///
    /// ```text
    /// { brushDef { ( p ) ( p ) ( p ) ( ( a b c ) ( d e f ) ) shader C S V ... } }
    /// {          ( p ) ( p ) ( p ) shader 0 0 0 0.5 0.5 C S V ...              }
    /// ```
    ///
    /// The alignment matrix becomes the legacy `offset offset rotation scaleX
    /// scaleY` quintet at its identity value. That discards texture alignment,
    /// which this wave does not use: C7 puts textures out of scope, and the
    /// render mesh colours faces by shader name.
    fn brush_def(&mut self, tokens: &[Tok], start: usize) -> usize {
        let end = skip_balanced(tokens, start);
        // start: '{', start+1: 'brushDef', start+2: '{' ... two closing braces.
        let inner_start = start + 3;
        let inner_end = end.saturating_sub(2);
        if tokens.get(start + 2) != Some(&Tok::LBrace) || inner_end < inner_start {
            // Not the shape we expected; hand it to the parser unchanged rather
            // than guessing.
            for tok in &tokens[start..end] {
                self.push(tok);
            }
            return end;
        }

        self.push(&Tok::LBrace);
        let mut i = inner_start;
        while i < inner_end {
            match face(tokens, i, inner_end) {
                Some((rewritten, next)) => {
                    for tok in &rewritten {
                        self.push(tok);
                    }
                    self.newline();
                    i = next;
                }
                None => {
                    self.push(&tokens[i]);
                    i += 1;
                }
            }
        }
        self.push(&Tok::RBrace);
        end
    }
}

/// Read one `brushDef` face, returning it in legacy form.
fn face(tokens: &[Tok], start: usize, limit: usize) -> Option<(Vec<Tok>, usize)> {
    let mut out = Vec::with_capacity(20);
    let mut i = start;

    // Three plane points, copied through verbatim.
    for _ in 0..3 {
        let (point, next) = paren_group(tokens, i, limit)?;
        out.extend(point);
        i = next;
    }

    // The texture-alignment matrix: `( ( a b c ) ( d e f ) )`, discarded.
    let (_matrix, next) = paren_group(tokens, i, limit)?;
    i = next;

    // The shader name.
    let Some(Tok::Word(shader)) = tokens.get(i) else {
        return None;
    };
    out.push(Tok::Word(shader.clone()));
    i += 1;

    // Legacy alignment at identity: no offset, no rotation, unit scale.
    for v in ["0", "0", "0", "0.5", "0.5"] {
        out.push(Tok::Word(v.to_string()));
    }

    // Whatever trailing numbers the face carried — Quake 3's contents, surface
    // flags and value. Kept: `texture.rs` reads them.
    while let Some(Tok::Word(w)) = tokens.get(i) {
        if i >= limit || !is_number(w) {
            break;
        }
        out.push(Tok::Word(w.clone()));
        i += 1;
    }

    Some((out, i))
}

/// Read a balanced `( ... )` group starting at `start`, tokens included.
fn paren_group(tokens: &[Tok], start: usize, limit: usize) -> Option<(Vec<Tok>, usize)> {
    if tokens.get(start) != Some(&Tok::LParen) {
        return None;
    }
    let mut depth = 0usize;
    let mut out = Vec::new();
    let mut i = start;
    while i < limit {
        match &tokens[i] {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            _ => {}
        }
        out.push(tokens[i].clone());
        i += 1;
        if depth == 0 {
            return Some((out, i));
        }
    }
    None
}

/// Whether a bare word is a number, so the trailing Quake 3 flag fields can be
/// told apart from the start of the next face.
fn is_number(word: &str) -> bool {
    let mut chars = word.chars();
    let first = chars.next();
    matches!(first, Some(c) if c.is_ascii_digit() || c == '-' || c == '+' || c == '.')
        && word
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
}

/// Index just past the brace group starting at `start`.
///
/// Returns `tokens.len()` for an unterminated group, which is what makes a
/// truncated file terminate rather than loop.
fn skip_balanced(tokens: &[Tok], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < tokens.len() {
        match tokens[i] {
            Tok::LBrace => depth += 1,
            Tok::RBrace => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    tokens.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_with_neither_construct_is_returned_untouched() {
        let src = "// entity 0\n{\n\"classname\" \"worldspawn\"\n}\n";
        let out = prepare(src);
        assert_eq!(out.text, src, "the fast path must not renumber lines");
        assert_eq!(out.patches_dropped, 0);
        assert_eq!(out.brush_defs, 0);
    }

    #[test]
    fn a_patch_is_dropped_and_counted() {
        let src = r#"
{
"classname" "worldspawn"
{
patchDef2
{
common/caulk
( 3 3 0 0 0 )
(
( ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) )
( ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) )
( ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) ( 0 0 0 0 0 ) )
)
}
}
}
"#;
        let out = prepare(src);
        assert_eq!(out.patches_dropped, 1);
        assert!(!out.text.contains("patchDef"), "got: {}", out.text);
        assert!(out.text.contains("worldspawn"), "the entity survives");
    }

    #[test]
    fn a_brush_def_becomes_a_legacy_brush() {
        let src = r#"
{
"classname" "worldspawn"
{
brushDef
{
( 128 0 0 ) ( 0 0 0 ) ( 0 128 0 ) ( ( 0.0078125 0 0 ) ( 0 0.0078125 0 ) ) common/caulk 0 0 0
( 0 0 128 ) ( 0 128 128 ) ( 128 0 128 ) ( ( 0.0078125 0 0 ) ( 0 0.0078125 0 ) ) base/floor 0 2 0
}
}
}
"#;
        let out = prepare(src);
        assert_eq!(out.brush_defs, 1);
        assert!(!out.text.contains("brushDef"));
        // The plane points survive verbatim, the matrix is gone, and a legacy
        // alignment quintet is in its place.
        assert!(
            out.text
                .contains("( 128 0 0 ) ( 0 0 0 ) ( 0 128 0 ) common/caulk 0 0 0 0.5 0.5 0 0 0"),
            "got: {}",
            out.text
        );
        // The Quake 3 flag triple is kept — `texture.rs` reads it.
        assert!(
            out.text.contains("base/floor 0 0 0 0.5 0.5 0 2 0"),
            "got: {}",
            out.text
        );
    }

    #[test]
    fn an_ordinary_brush_beside_a_patch_survives_the_slow_path() {
        let src = r#"
{
"classname" "worldspawn"
{
( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) common/caulk 0 0 0 0.5 0.5
}
{
patchDef2
{
common/caulk
( 3 3 0 0 0 )
( )
}
}
}
"#;
        let out = prepare(src);
        assert_eq!(out.patches_dropped, 1);
        assert!(
            out.text
                .contains("( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) common/caulk 0 0 0 0.5 0.5"),
            "got: {}",
            out.text
        );
    }

    #[test]
    fn comments_and_quoted_values_survive_lexing() {
        let toks = lex("// a comment\n\"key\" \"a value with spaces\" {}");
        assert_eq!(
            toks,
            vec![
                Tok::Quoted("key".into()),
                Tok::Quoted("a value with spaces".into()),
                Tok::LBrace,
                Tok::RBrace,
            ]
        );
    }

    #[test]
    fn an_unterminated_brace_group_terminates() {
        // A truncated download must produce a parse error, not a hang.
        let out = prepare("{ \"classname\" \"worldspawn\" { brushDef { ( 0 0 0 )");
        assert!(!out.text.is_empty());
    }

    #[test]
    fn numbers_are_told_apart_from_shader_names() {
        assert!(is_number("-64"));
        assert!(is_number("0.5"));
        assert!(is_number("1e-3"));
        assert!(!is_number("common/caulk"));
        assert!(!is_number("base_floor/clang"));
    }
}
