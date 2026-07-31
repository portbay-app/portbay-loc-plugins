//! `@portbay/swc-plugin-loc` — SWC/Turbopack port of the babel/vite loc stamper.
//!
//! Stamps each JSX opening element with its authored source location, anchored
//! at the opening `<` in the *authored* source file. Exact parity with
//! `@portbay/babel-plugin-loc` / `@portbay/vite-plugin-loc`:
//!
//!   * Host element (lowercase intrinsic `<div>`, `<button>`) —
//!     `data-pb-loc="<relpath>:<line>:<col>"` (renders a DOM node).
//!   * Component call site (uppercase `<Hero title="…" />`) —
//!     `data-pb-comp="<relpath>:<line>:<col>"` (the JSX call site).
//!
//! The two use DISTINCT attribute names on purpose. A component renders no DOM
//! node of its own, so `data-pb-comp` rides as a *prop*: React copies enumerable
//! props into the component fiber's `memoizedProps`, so PortBay's editor reads
//! the call-site coordinate off the resolved fiber at runtime — a genuine SOURCE
//! coordinate needing no runtime sourcemap reversal. PortBay's Rust resolver
//! reads either attribute to map a rendered node back to its precise source
//! span. See the repo README for the contract.
//!
//! Member (`<Foo.Bar>`) and namespaced (`<svg:rect>`) tags are skipped — an
//! explicit v1 gap for member/namespaced components.
//!
//! Dev-only by design: PortBay only wires the plugin into `next dev`. The gate
//! also honours an explicit `enabled` config flag and, when that is unset, the
//! host's `NODE_ENV` (production => off).

use std::path::Path;

use serde::Deserialize;
use swc_core::common::{SourceMapper, Span, DUMMY_SP};
use swc_core::ecma::ast::{
    IdentName, JSXAttr, JSXAttrName, JSXAttrOrSpread, JSXAttrValue, JSXElementName,
    JSXOpeningElement, Str,
};
use swc_core::ecma::visit::{VisitMut, VisitMutWith};

const ATTR_HOST: &str = "data-pb-loc";
const ATTR_COMP: &str = "data-pb-comp";

/// Plugin config, JSON-decoded from the `.swcrc` / `next.config`
/// `experimental.swcPlugins` entry.
#[derive(Debug, Default, Deserialize)]
pub struct Config {
    /// Project root. Only files *under* it are stamped, and the emitted path is
    /// relative to it (POSIX). Defaults to empty => stamp nothing (safe).
    #[serde(default)]
    pub root: Option<String>,
    /// Hard on/off switch. When unset, the host's `NODE_ENV` decides
    /// (`production` => off, anything else => on).
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Resolve the dev-only gate: explicit `enabled` wins, else `NODE_ENV` (only
/// `"production"` turns it off). Mirrors the babel plugin's `isEnabled`, minus
/// the `PORTBAY_LOC` env escape hatch (the wasm sandbox can't read process env;
/// PortBay passes `enabled` through config instead).
pub fn resolve_enabled(enabled_cfg: Option<bool>, env: Option<&str>) -> bool {
    enabled_cfg.unwrap_or(env != Some("production"))
}

/// Why [`rel_posix_checked`] refused to produce a stamp path. Carried so the
/// plugin entry point can say *which* bail happened instead of no-oping in
/// silence — a silent bail is exactly how the Turbopack breakage survived
/// unnoticed (every file transformed, zero elements stamped, HTTP 200).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelRefusal {
    /// No `root` in the plugin config (the default is "stamp nothing").
    NoRoot,
    /// The host gave us no filename for this module.
    NoFilename,
    /// An absolute `filename` that does not live under `root`.
    OutsideRoot,
    /// A relative `filename` that climbs out of `root` with `..`.
    EscapesRoot,
    /// `filename` resolved to `root` itself — no file component to stamp.
    EmptyPath,
}

impl RelRefusal {
    /// One-line explanation, ready to hang off a diagnostic.
    pub fn why(self) -> &'static str {
        match self {
            RelRefusal::NoRoot => {
                "no `root` was configured, so every file is treated as outside the project"
            }
            RelRefusal::NoFilename => "the bundler supplied no filename for this module",
            RelRefusal::OutsideRoot => "the file is an absolute path outside `root`",
            RelRefusal::EscapesRoot => "the file is a relative path that climbs above `root`",
            RelRefusal::EmptyPath => "the file resolved to `root` itself, leaving no relative path",
        }
    }
}

/// Is `p` (already POSIX-normalised) an absolute path? A leading `/`, or a
/// Windows drive prefix (`C:/…`). The wasm target's [`Path`] only understands
/// POSIX roots, so the drive form is recognised explicitly rather than being
/// left to `Path::is_absolute` — otherwise a Windows absolute path would be
/// mistaken for a relative one and emitted verbatim.
fn is_absolute_pathish(p: &str) -> bool {
    if p.starts_with('/') {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Project-root-relative POSIX path, or a [`RelRefusal`] saying why not.
///
/// Bundlers disagree about what `filename` is:
///
///   * **webpack** (and `swc` directly) hands over an ABSOLUTE path
///     (`/proj/app/page.tsx`) — strip `root` off the front.
///   * **Turbopack** hands over a path that is ALREADY project-root-relative
///     (`app/page.tsx`) — use it as-is. Feeding that to `strip_prefix` returns
///     `Err`, which is what made this plugin stamp exactly nothing on every
///     default Next 16 app.
///
/// Either way the result is normalised to POSIX separators and can never
/// contain a `..` component: PortBay's resolver rejects those outright, so a
/// path escaping `root` must refuse rather than emit something unusable.
pub fn rel_posix_checked(root: &str, filename: &str) -> Result<String, RelRefusal> {
    if filename.is_empty() {
        return Err(RelRefusal::NoFilename);
    }
    if root.is_empty() {
        return Err(RelRefusal::NoRoot);
    }
    // Normalise separators first so a Windows-shaped path is compared and
    // emitted in the same alphabet as a POSIX one.
    let file = filename.replace('\\', "/");
    let root = root.replace('\\', "/");
    // A trailing slash on `root` would break component-wise prefix matching.
    let root_trimmed = root.trim_end_matches('/');
    let root = if root_trimmed.is_empty() {
        root.as_str()
    } else {
        root_trimmed
    };

    let rel: String = if is_absolute_pathish(&file) {
        // Absolute (webpack): must genuinely live under `root`. `strip_prefix`
        // is component-wise, so `/proj` never matches `/project-x`.
        match Path::new(&file).strip_prefix(root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => return Err(RelRefusal::OutsideRoot),
        }
    } else {
        // Already relative (Turbopack): root-relative by definition.
        file.clone()
    };

    // Drop no-op `.` segments, collapse doubled slashes, and resolve `..`
    // against what we have so far — refusing the moment one would climb above
    // `root`. The emitted path therefore never contains `..`, which PortBay's
    // resolver rejects outright.
    let mut parts: Vec<&str> = Vec::new();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(RelRefusal::EscapesRoot);
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(RelRefusal::EmptyPath);
    }
    Ok(parts.join("/"))
}

/// Project-root-relative POSIX path, or `None` when `filename` cannot be
/// expressed as one (we never emit a `..` path the resolver would reject).
/// Mirrors the babel plugin's `relPosix`. See [`rel_posix_checked`] for the
/// reason behind a `None`.
pub fn rel_posix(root: &str, filename: &str) -> Option<String> {
    rel_posix_checked(root, filename).ok()
}

/// The stamp attribute for a JSX tag, or `None` when the tag is skipped.
///
///   * lowercase plain `JSXIdentifier` (`<div>`, `<my-widget>`) => host DOM
///     node => `data-pb-loc`.
///   * uppercase plain `JSXIdentifier` (`<Hero>`) => Component call site =>
///     `data-pb-comp`.
///   * member (`<Foo.Bar>`) / namespaced (`<svg:rect>`) => `None` (v1 gap).
fn stamp_attr(name: &JSXElementName) -> Option<&'static str> {
    match name {
        JSXElementName::Ident(ident) => match ident.sym.as_ref().chars().next() {
            Some(c) if c.is_ascii_lowercase() => Some(ATTR_HOST),
            Some(_) => Some(ATTR_COMP),
            None => None,
        },
        _ => None,
    }
}

/// Idempotency guard: is this element already stamped with `attr`? Host and
/// component names are distinct, so they never collide.
fn has_loc_attr(el: &JSXOpeningElement, attr: &str) -> bool {
    el.attrs.iter().any(|a| {
        matches!(
            a,
            JSXAttrOrSpread::JSXAttr(JSXAttr {
                name: JSXAttrName::Ident(n),
                ..
            }) if n.sym.as_ref() == attr
        )
    })
}

/// The AST transform. Generic over the source map so the native test harness
/// (a real [`swc_core::common::SourceMap`]) and the wasm plugin runtime (a
/// `PluginSourceMapProxy`) drive the exact same code.
pub struct LocVisitor<'a, S: SourceMapper> {
    source_map: &'a S,
    /// Pre-resolved root-relative path; `None` => stamp nothing for this file.
    rel: Option<String>,
    enabled: bool,
}

impl<'a, S: SourceMapper> LocVisitor<'a, S> {
    pub fn new(source_map: &'a S, rel: Option<String>, enabled: bool) -> Self {
        Self {
            source_map,
            rel,
            enabled,
        }
    }

    fn loc_value(&self, span: Span) -> Option<String> {
        let rel = self.rel.as_ref()?;
        let loc = self.source_map.lookup_char_pos(span.lo);
        // Babel emits 1-based line + 1-based column of the opening `<`.
        // SWC's `Loc.line` is already 1-based; `Loc.col` is a 0-based CharPos.
        Some(format!("{rel}:{}:{}", loc.line, loc.col.0 + 1))
    }
}

impl<S: SourceMapper> VisitMut for LocVisitor<'_, S> {
    fn visit_mut_jsx_opening_element(&mut self, el: &mut JSXOpeningElement) {
        // Descend first so JSX nested inside attribute values is stamped too.
        el.visit_mut_children_with(self);

        if !self.enabled || self.rel.is_none() {
            return;
        }
        let Some(attr) = stamp_attr(&el.name) else {
            return;
        };
        if has_loc_attr(el, attr) {
            return;
        }
        let Some(value) = self.loc_value(el.span) else {
            return;
        };

        // Appended *last* so a `{...spread}` can never override the loc — same
        // ordering guarantee the babel plugin makes.
        el.attrs.push(JSXAttrOrSpread::JSXAttr(JSXAttr {
            span: DUMMY_SP,
            name: JSXAttrName::Ident(IdentName::new(attr.into(), DUMMY_SP)),
            value: Some(JSXAttrValue::Str(Str {
                span: DUMMY_SP,
                value: value.into(),
                raw: None,
            })),
        }));
    }
}

// ---------------------------------------------------------------------------
// Plugin entry point (wasm). Tests below exercise `LocVisitor` directly.
// ---------------------------------------------------------------------------

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};

use swc_core::ecma::ast::Program;
use swc_core::plugin::errors::HANDLER;
use swc_core::plugin::{
    metadata::TransformPluginMetadataContextKind, plugin_transform,
    proxies::TransformPluginProgramMetadata,
};

/// One bit per [`RelRefusal`] variant, to keep a misconfiguration from being
/// reported more than once for the same reason.
///
/// MEASURED CAVEAT: swc's plugin runner instantiates a fresh wasm module per
/// transform, so this static is reset between files and the warning does in
/// practice fire once per refused file, not once per build. There is no
/// plugin-side state that survives a transform, so de-duplication across files
/// is not achievable from in here — and a `root` misconfiguration refuses every
/// file anyway, which is the condition worth being loud about. The guard is
/// kept because it is correct for any host that does reuse an instance.
static WARNED: AtomicU8 = AtomicU8::new(0);

fn refusal_bit(r: RelRefusal) -> u8 {
    match r {
        RelRefusal::NoRoot => 1,
        RelRefusal::NoFilename => 1 << 1,
        RelRefusal::OutsideRoot => 1 << 2,
        RelRefusal::EscapesRoot => 1 << 3,
        RelRefusal::EmptyPath => 1 << 4,
    }
}

/// Emit a build warning naming the bail — the thing this plugin used to do
/// silently, which is how a dead feature shipped unnoticed on every default
/// Next 16 app.
///
/// Two channels, because neither alone is reliable:
///   * `HANDLER` — the structured, documented plugin diagnostic channel. It
///     reaches the host through `__emit_diagnostics`, but whether the host
///     PRINTS a warning is the host's decision (verified: `@swc/core`'s Node
///     binding buffers it and discards it when the transform succeeds).
///   * the wasm sandbox's stderr — verified to land on the user's terminal
///     through `@swc/core`.
///
/// `HANDLER` is installed by the `#[plugin_transform]` wrapper, so this is only
/// ever called from inside the transform — never from the library functions the
/// native test harness drives.
fn warn_once(reason: RelRefusal, root: &str, filename: &str) {
    let bit = refusal_bit(reason);
    if WARNED.fetch_or(bit, Ordering::Relaxed) & bit != 0 {
        return;
    }
    let root = if root.is_empty() { "(unset)" } else { root };
    let filename = if filename.is_empty() {
        "(unset)"
    } else {
        filename
    };
    let msg = format!(
        "@portbay/swc-plugin-loc: not stamping — {}. root={root:?} filename={filename:?}. \
         No `data-pb-loc` will be emitted for these files, so PortBay's visual editor cannot \
         resolve them to source. Set `root` to the project directory the bundler's filenames \
         are relative to (usually `process.cwd()`).",
        reason.why()
    );
    HANDLER.with(|handler| handler.warn(&msg));
    // Belt and braces: the host decides whether a plugin's `HANDLER` warning is
    // ever printed (`@swc/core`'s Node binding buffers diagnostics and drops
    // them when the transform succeeds), so also write the line to the wasm
    // sandbox's stderr. `write_all` is used rather than `eprintln!` because the
    // latter PANICS when the host has not wired up fd 2 — here a closed stderr
    // is simply a no-op.
    let _ = std::io::stderr().write_all(format!("warning: {msg}\n").as_bytes());
}

#[plugin_transform]
fn process_transform(mut program: Program, metadata: TransformPluginProgramMetadata) -> Program {
    let config: Config = metadata
        .get_transform_plugin_config()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    let filename = metadata
        .get_context(&TransformPluginMetadataContextKind::Filename)
        .unwrap_or_default();
    let env = metadata.get_context(&TransformPluginMetadataContextKind::Env);

    let enabled = resolve_enabled(config.enabled, env.as_deref());
    let root = config.root.unwrap_or_default();
    let rel = match rel_posix_checked(&root, &filename) {
        Ok(rel) => Some(rel),
        Err(reason) => {
            // Only complain when the plugin was actually meant to be running;
            // a deliberate `enabled: false` (or a production build) is not a
            // failure, it is the gate working.
            if enabled {
                warn_once(reason, &root, &filename);
            }
            None
        }
    };

    let mut visitor = LocVisitor::new(&metadata.source_map, rel, enabled);
    program.visit_mut_with(&mut visitor);
    program
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::common::sync::Lrc;
    use swc_core::common::{FileName, SourceMap};
    use swc_core::ecma::codegen::{text_writer::JsWriter, Config as CodegenConfig, Emitter};
    use swc_core::ecma::parser::{lexer::Lexer, EsSyntax, Parser, StringInput, Syntax, TsSyntax};

    /// Which shape of `filename` the host hands the plugin. Webpack (and swc
    /// directly) pass an absolute path; Turbopack passes one that is already
    /// project-root-relative. Both must stamp identical coordinates.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Bundler {
        /// `/proj/src/App.jsx`
        Webpack,
        /// `src/App.jsx`
        Turbopack,
    }

    struct Opts {
        file: &'static str,
        root: &'static str,
        enabled: bool,
        ts: bool,
        bundler: Bundler,
    }

    impl Default for Opts {
        fn default() -> Self {
            Opts {
                file: "src/App.jsx",
                root: "/proj",
                enabled: true,
                ts: false,
                bundler: Bundler::Webpack,
            }
        }
    }

    /// Parse `src` as JSX/TSX, run the visitor, emit code back to a string —
    /// the same shape as the babel package's vitest `run()` helper.
    fn run(src: &str, opts: Opts) -> String {
        let cm: Lrc<SourceMap> = Default::default();
        let abs = format!("{}/{}", opts.root, opts.file);
        // Exactly what `TransformPluginMetadataContextKind::Filename` would
        // carry under each bundler.
        let host_filename = match opts.bundler {
            Bundler::Webpack => abs.clone(),
            Bundler::Turbopack => opts.file.to_string(),
        };
        let fm = cm.new_source_file(Lrc::new(FileName::Real(abs.clone().into())), src.to_string());

        let syntax = if opts.ts {
            Syntax::Typescript(TsSyntax {
                tsx: true,
                ..Default::default()
            })
        } else {
            Syntax::Es(EsSyntax {
                jsx: true,
                ..Default::default()
            })
        };
        let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
        let mut parser = Parser::new_from(lexer);
        let mut module = parser.parse_module().expect("parse");

        let rel = rel_posix(opts.root, &host_filename);
        let mut v = LocVisitor::new(&*cm, rel, opts.enabled);
        module.visit_mut_with(&mut v);

        let mut buf = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default(),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter.emit_module(&module).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    /// All values of `attr` in emit order.
    fn all_of(out: &str, attr: &str) -> Vec<String> {
        let needle = format!("{attr}=\"");
        let mut locs = Vec::new();
        let mut rest = out;
        while let Some(i) = rest.find(&needle) {
            rest = &rest[i + needle.len()..];
            if let Some(j) = rest.find('"') {
                locs.push(rest[..j].to_string());
                rest = &rest[j + 1..];
            } else {
                break;
            }
        }
        locs
    }

    /// All `data-pb-loc` (host) values in emit order.
    fn all_locs(out: &str) -> Vec<String> {
        all_of(out, "data-pb-loc")
    }

    /// All `data-pb-comp` (component call-site) values in emit order.
    fn all_comps(out: &str) -> Vec<String> {
        all_of(out, "data-pb-comp")
    }

    /// Every emitted loc must point exactly at a `<` in the ORIGINAL source —
    /// the property PortBay's Rust resolver depends on.
    fn assert_anchor_at_bracket(loc: &str, src: &str) {
        let mut it = loc.rsplitn(3, ':');
        let col: usize = it.next().unwrap().parse().expect("col");
        let line: usize = it.next().unwrap().parse().expect("line");
        let l = src.lines().nth(line - 1).expect("line in source");
        assert_eq!(
            l.as_bytes()[col - 1],
            b'<',
            "loc {loc} must anchor at '<' (line: {l:?})"
        );
    }

    #[test]
    fn stamps_host_and_anchors_col_at_bracket() {
        let code = "export default function App() {\n  return <div className=\"card\">Hi</div>;\n}";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 1);
        assert!(locs[0].starts_with("src/App.jsx:2:"));
        assert_anchor_at_bracket(&locs[0], code);
        // className survives — this is what unlocks React class write-back.
        assert!(out.contains("className=\"card\""));
    }

    #[test]
    fn stamps_host_with_loc_and_component_with_comp() {
        let code = "const App = () => (\n  <Hero>\n    <span>x</span>\n  </Hero>\n);";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        let comps = all_comps(&out);
        assert_eq!(locs.len(), 1, "only <span> is a host");
        assert_eq!(comps.len(), 1, "only <Hero> is a Component");
        // The two attributes never cross: a Component gets no data-pb-loc and a
        // host gets no data-pb-comp.
        assert!(!out.contains("Hero data-pb-loc"));
        assert!(!out.contains("span data-pb-comp"));
        assert_anchor_at_bracket(&locs[0], code);
        assert_anchor_at_bracket(&comps[0], code);
    }

    #[test]
    fn stamps_component_call_site_at_bracket() {
        let code =
            "export default function App() {\n  return <Hero title=\"Welcome\" count={3} disabled />;\n}";
        let out = run(code, Opts::default());
        let comps = all_comps(&out);
        assert_eq!(comps.len(), 1);
        assert!(comps[0].starts_with("src/App.jsx:2:"));
        assert_anchor_at_bracket(&comps[0], code);
        // Authored props survive verbatim — the source the prop classifier reads.
        assert!(out.contains("title=\"Welcome\""));
        assert!(out.contains("count={3}"));
    }

    #[test]
    fn skips_member_and_namespaced_tags() {
        let code = "const A = () => (\n  <Foo.Bar>\n    <ns:widget />\n  </Foo.Bar>\n);";
        let out = run(code, Opts::default());
        assert_eq!(all_locs(&out).len(), 0);
        assert_eq!(all_comps(&out).len(), 0);
    }

    #[test]
    fn component_stamp_is_idempotent() {
        let code = "const A = () => <Hero title=\"x\" />;";
        let once = run(code, Opts::default());
        let twice = run(&once, Opts::default());
        assert_eq!(all_comps(&twice).len(), 1);
    }

    #[test]
    fn stamps_nested_host_independently() {
        let code = "const A = () => <ul><li>a</li><li>b</li></ul>;";
        let out = run(code, Opts::default());
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 3, "ul + 2 li");
        for l in &locs {
            assert_anchor_at_bracket(l, code);
        }
    }

    #[test]
    fn map_element_yields_one_source_loc() {
        let code =
            "const List = ({ items }) => (\n  <ul>{items.map((it) => <li key={it.id}>{it.label}</li>)}</ul>\n);";
        let out = run(code, Opts::default());
        // <ul> + one source <li> => 2 locs; the rendered copies are runtime.
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 2);
        let li_stamps = out.matches("<li").count();
        let li_with_loc = out
            .match_indices("<li")
            .filter(|(i, _)| {
                out[*i..]
                    .split_once('>')
                    .map(|(open, _)| open.contains("data-pb-loc"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(li_stamps, 1, "one <li> in source");
        assert_eq!(li_with_loc, 1);
        for l in &locs {
            assert_anchor_at_bracket(l, code);
        }
    }

    #[test]
    fn does_not_mistake_tsx_generics_for_elements() {
        let code = "const C = () => {\n  const v = useState<Record<string, number>>({});\n  return <div>{v}</div>;\n};";
        let out = run(
            code,
            Opts {
                file: "src/C.tsx",
                ts: true,
                ..Default::default()
            },
        );
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 1, "only <div>");
        assert!(locs[0].starts_with("src/C.tsx:"));
        assert!(out.contains("useState"));
    }

    #[test]
    fn is_idempotent() {
        let code = "const A = () => <div>x</div>;";
        let once = run(code, Opts::default());
        let twice = run(&once, Opts::default());
        assert_eq!(all_locs(&twice).len(), 1);
    }

    #[test]
    fn emits_nothing_when_disabled() {
        let code = "const A = () => <div>x</div>;";
        let out = run(
            code,
            Opts {
                enabled: false,
                ..Default::default()
            },
        );
        assert_eq!(all_locs(&out).len(), 0);
    }

    #[test]
    fn skips_files_outside_root() {
        let rel = rel_posix("/proj", "/elsewhere/App.jsx");
        assert!(rel.is_none());

        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Real("/elsewhere/App.jsx".into())),
            "const A = () => <div>x</div>;".to_string(),
        );
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax {
                jsx: true,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        let mut parser = Parser::new_from(lexer);
        let mut module = parser.parse_module().unwrap();
        let mut v = LocVisitor::new(&*cm, rel, true);
        module.visit_mut_with(&mut v);

        let mut buf = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default(),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter.emit_module(&module).unwrap();
        }
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(all_locs(&out).len(), 0);
    }

    #[test]
    fn rel_posix_contract() {
        assert_eq!(
            rel_posix("/proj", "/proj/src/App.jsx").as_deref(),
            Some("src/App.jsx")
        );
        assert_eq!(rel_posix("/proj", "/elsewhere/App.jsx"), None);
        assert_eq!(rel_posix("/proj", "/proj"), None); // empty rel
        assert_eq!(rel_posix("", "/proj/x.jsx"), None);
        assert_eq!(rel_posix("/proj", ""), None);
    }

    // ------------------------------------------------- Turbopack filename shape
    //
    // Turbopack hands the plugin a filename that is ALREADY project-root-
    // relative (`app/page.tsx`); webpack hands over an absolute one. Feeding the
    // relative form to `strip_prefix` returned `Err`, so the plugin stamped
    // nothing at all on every default Next 16 app — silently, with a 200
    // response. These tests pin BOTH shapes so neither can regress into the
    // other's bail.

    #[test]
    fn stamps_when_bundler_passes_a_relative_filename() {
        // The exact shape Turbopack gives a Next App Router page.
        let code = "export default function Page() {\n  return <div className=\"card\">Hi</div>;\n}";
        let out = run(
            code,
            Opts {
                file: "app/page.tsx",
                ts: true,
                bundler: Bundler::Turbopack,
                ..Default::default()
            },
        );
        let locs = all_locs(&out);
        assert_eq!(locs.len(), 1, "Turbopack's relative filename must stamp");
        // The `app/` prefix must survive — without it the resolver cannot find
        // the file on disk.
        assert!(
            locs[0].starts_with("app/page.tsx:2:"),
            "expected an app/-prefixed loc, got {:?}",
            locs[0]
        );
        assert_anchor_at_bracket(&locs[0], code);
    }

    #[test]
    fn absolute_and_relative_filenames_stamp_identical_coordinates() {
        let code = "const A = () => (\n  <Hero>\n    <span>x</span>\n  </Hero>\n);";
        let webpack = run(
            code,
            Opts {
                file: "app/page.tsx",
                ts: true,
                bundler: Bundler::Webpack,
                ..Default::default()
            },
        );
        let turbopack = run(
            code,
            Opts {
                file: "app/page.tsx",
                ts: true,
                bundler: Bundler::Turbopack,
                ..Default::default()
            },
        );
        assert_eq!(all_locs(&webpack), all_locs(&turbopack));
        assert_eq!(all_comps(&webpack), all_comps(&turbopack));
        assert_eq!(all_locs(&webpack), vec!["app/page.tsx:3:5"]);
        assert_eq!(all_comps(&webpack), vec!["app/page.tsx:2:3"]);
    }

    #[test]
    fn relative_filename_escaping_root_still_refuses() {
        // A relative path that climbs out of the project must NOT become a
        // stamp: the resolver rejects any `..`, so emitting one would turn a
        // safety refusal into an unusable coordinate.
        assert_eq!(rel_posix("/proj", "../secrets/App.jsx"), None);
        assert_eq!(rel_posix("/proj", "app/../../App.jsx"), None);

        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            Lrc::new(FileName::Real("../secrets/App.jsx".into())),
            "const A = () => <div>x</div>;".to_string(),
        );
        let lexer = Lexer::new(
            Syntax::Es(EsSyntax {
                jsx: true,
                ..Default::default()
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        let mut parser = Parser::new_from(lexer);
        let mut module = parser.parse_module().unwrap();
        let mut v = LocVisitor::new(&*cm, rel_posix("/proj", "../secrets/App.jsx"), true);
        module.visit_mut_with(&mut v);

        let mut buf = Vec::new();
        {
            let wr = JsWriter::new(cm.clone(), "\n", &mut buf, None);
            let mut emitter = Emitter {
                cfg: CodegenConfig::default(),
                cm: cm.clone(),
                comments: None,
                wr,
            };
            emitter.emit_module(&module).unwrap();
        }
        assert_eq!(all_locs(&String::from_utf8(buf).unwrap()).len(), 0);
    }

    #[test]
    fn rel_posix_checked_reports_why_it_refused() {
        use RelRefusal::*;
        assert_eq!(rel_posix_checked("/proj", ""), Err(NoFilename));
        assert_eq!(rel_posix_checked("", "app/page.tsx"), Err(NoRoot));
        assert_eq!(
            rel_posix_checked("/proj", "/elsewhere/App.jsx"),
            Err(OutsideRoot)
        );
        assert_eq!(rel_posix_checked("/proj", "../up.jsx"), Err(EscapesRoot));
        assert_eq!(rel_posix_checked("/proj", "/proj"), Err(EmptyPath));
        assert_eq!(rel_posix_checked("/proj", "./"), Err(EmptyPath));
        // Every refusal carries a human explanation for the build warning.
        for r in [NoRoot, NoFilename, OutsideRoot, EscapesRoot, EmptyPath] {
            assert!(!r.why().is_empty());
        }
    }

    #[test]
    fn rel_posix_normalises_separators_and_noise() {
        // Turbopack shapes.
        assert_eq!(
            rel_posix("/proj", "app/page.tsx").as_deref(),
            Some("app/page.tsx")
        );
        assert_eq!(
            rel_posix("/proj", "./app/page.tsx").as_deref(),
            Some("app/page.tsx")
        );
        assert_eq!(
            rel_posix("/proj", "app//nested/page.tsx").as_deref(),
            Some("app/nested/page.tsx")
        );
        // A trailing slash on `root` must not break prefix matching.
        assert_eq!(
            rel_posix("/proj/", "/proj/app/page.tsx").as_deref(),
            Some("app/page.tsx")
        );
        // Component-wise matching: `/proj` must not claim `/project-x`.
        assert_eq!(rel_posix("/proj", "/project-x/app/page.tsx"), None);
        // Windows shapes normalise to POSIX; the drive form counts as absolute
        // even though the wasm target's `Path` has no concept of it.
        assert_eq!(
            rel_posix("C:\\proj", "C:\\proj\\app\\page.tsx").as_deref(),
            Some("app/page.tsx")
        );
        assert_eq!(
            rel_posix("C:/proj", "app\\page.tsx").as_deref(),
            Some("app/page.tsx")
        );
        assert_eq!(rel_posix("C:/proj", "D:/other/page.tsx"), None);
    }

    #[test]
    fn resolve_enabled_contract() {
        assert!(!resolve_enabled(None, Some("production")));
        assert!(resolve_enabled(None, Some("development")));
        assert!(resolve_enabled(None, None));
        assert!(resolve_enabled(Some(true), Some("production")));
        assert!(!resolve_enabled(Some(false), Some("development")));
    }
}
